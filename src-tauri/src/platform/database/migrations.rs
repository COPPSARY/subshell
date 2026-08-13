use include_dir::{Dir, include_dir};
use rusqlite::Connection;

use super::DatabaseError;

static DIRECTORY: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

#[derive(Clone, Copy)]
pub(super) struct Migration {
    version: u32,
    sql: &'static str,
}

pub(super) fn embedded() -> Result<Vec<Migration>, DatabaseError> {
    let mut migrations = DIRECTORY
        .files()
        .map(|file| {
            let name = file
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    DatabaseError::InvalidMigration("migration filename is not UTF-8".into())
                })?;
            let (prefix, _) = name.split_once('_').ok_or_else(|| {
                DatabaseError::InvalidMigration(format!("{name} must start with NNNN_"))
            })?;
            let version = prefix.parse::<u32>().map_err(|_| {
                DatabaseError::InvalidMigration(format!("{name} has an invalid version"))
            })?;
            let sql = file
                .contents_utf8()
                .ok_or_else(|| DatabaseError::InvalidMigration(format!("{name} is not UTF-8")))?;
            Ok(Migration { version, sql })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;

    migrations.sort_by_key(|migration| migration.version);
    for (index, migration) in migrations.iter().enumerate() {
        let expected = index as u32 + 1;
        if migration.version != expected {
            return Err(DatabaseError::InvalidMigration(format!(
                "expected migration {expected:04}, found {:04}",
                migration.version
            )));
        }
    }
    Ok(migrations)
}

pub(super) fn apply(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), DatabaseError> {
    let current: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = migrations.last().map_or(0, |migration| migration.version);
    if current > supported {
        return Err(DatabaseError::NewerSchema {
            found: current,
            supported,
        });
    }

    for migration in migrations
        .iter()
        .filter(|migration| migration.version > current)
    {
        let transaction = connection.transaction()?;
        transaction.execute_batch(migration.sql)?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_numbered_migrations_without_a_registry() {
        let migrations = embedded().unwrap();
        assert_eq!(
            migrations
                .iter()
                .map(|migration| migration.version)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]
        );
    }

    #[test]
    fn forward_migration_preserves_rows() {
        let mut connection = Connection::open_in_memory().unwrap();
        let migrations = embedded().unwrap();
        apply(&mut connection, &migrations[..1]).unwrap();
        connection
            .execute(
                "INSERT INTO projects (id, name, path, created_at, updated_at) \
             VALUES ('p1', 'Fixture', '/tmp/fixture', 'now', 'now')",
                [],
            )
            .unwrap();
        apply(&mut connection, &migrations).unwrap();

        assert_eq!(
            connection
                .query_row::<String, _, _>("SELECT name FROM projects WHERE id = 'p1'", [], |row| {
                    row.get(0)
                },)
                .unwrap(),
            "Fixture"
        );
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            13
        );
    }

    #[test]
    fn duplicate_unused_inherited_profiles_are_soft_removed() {
        let mut connection = Connection::open_in_memory().unwrap();
        let migrations = embedded().unwrap();
        apply(&mut connection, &migrations[..12]).unwrap();
        for id in ["preferred", "duplicate"] {
            connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES(?1,'codex','Codex',?2,'active','now','now')", rusqlite::params![id, format!("/tmp/{id}")]).unwrap();
            connection.execute("INSERT INTO generic_provider_profiles(provider_account_id,executable_path,arguments_json,resume_arguments_json,prompt_mode,config_root_env_var,inherit_user_home) VALUES(?1,'/usr/bin/codex','[\"{prompt}\"]','[\"resume\",\"--last\"]','argument','CODEX_HOME',1)", [id]).unwrap();
        }
        connection.execute("INSERT INTO app_settings(key,value,updated_at) VALUES('default_provider_account_id','preferred','now')", []).unwrap();

        apply(&mut connection, &migrations).unwrap();

        assert!(
            connection
                .query_row::<Option<String>, _, _>(
                    "SELECT removed_at FROM provider_accounts WHERE id='preferred'",
                    [],
                    |row| row.get(0)
                )
                .unwrap()
                .is_none()
        );
        assert!(
            connection
                .query_row::<Option<String>, _, _>(
                    "SELECT removed_at FROM provider_accounts WHERE id='duplicate'",
                    [],
                    |row| row.get(0)
                )
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn upgrades_detected_profiles_to_interactive_arguments() {
        let mut connection = Connection::open_in_memory().unwrap();
        let migrations = embedded().unwrap();
        apply(&mut connection, &migrations[..3]).unwrap();
        for (id, name, arguments) in [
            ("claude", "Claude Code", r#"["-p","{prompt}"]"#),
            ("codex", "Codex", r#"["exec","{prompt}"]"#),
            ("gemini", "Gemini CLI", r#"["-p","{prompt}"]"#),
        ] {
            connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES(?1,'generic',?2,?3,'active','now','now')", rusqlite::params![id,name,format!("/tmp/{id}")]).unwrap();
            connection.execute("INSERT INTO generic_provider_profiles(provider_account_id,executable_path,arguments_json,prompt_mode,inherit_user_home) VALUES(?1,?2,?3,'argument',1)", rusqlite::params![id,format!("/usr/bin/{id}"),arguments]).unwrap();
        }

        apply(&mut connection, &migrations).unwrap();

        let arguments = |id: &str| {
            connection.query_row::<String, _, _>("SELECT arguments_json FROM generic_provider_profiles WHERE provider_account_id=?1", [id], |row| row.get(0)).unwrap()
        };
        assert_eq!(arguments("codex"), r#"["{prompt}"]"#);
        assert_eq!(arguments("gemini"), r#"["-i","{prompt}"]"#);
        assert_eq!(
            arguments("claude"),
            r#"["--session-id","{sessionId}","{prompt}"]"#
        );
        assert_eq!(
            connection.query_row::<String, _, _>("SELECT resume_arguments_json FROM generic_provider_profiles WHERE provider_account_id='codex'", [], |row| row.get(0)).unwrap(),
            r#"["resume","--last"]"#
        );
    }

    #[test]
    fn failed_migration_rolls_back_without_advancing_the_version() {
        let mut connection = Connection::open_in_memory().unwrap();
        let mut broken = embedded().unwrap();
        apply(&mut connection, &broken).unwrap();
        broken.push(Migration {
            version: 14,
            sql: "CREATE TABLE partial (id TEXT); THIS IS NOT SQL;",
        });

        assert!(apply(&mut connection, &broken).is_err());
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            13
        );
        assert_eq!(
            connection
                .query_row::<u32, _, _>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'partial'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn rejects_a_database_newer_than_the_embedded_schema() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 14).unwrap();

        let error = apply(&mut connection, &embedded().unwrap()).unwrap_err();

        assert!(matches!(
            error,
            DatabaseError::NewerSchema {
                found: 14,
                supported: 13
            }
        ));
    }
}
