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
            vec![1, 2]
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
            2
        );
    }

    #[test]
    fn failed_migration_rolls_back_without_advancing_the_version() {
        let mut connection = Connection::open_in_memory().unwrap();
        let mut broken = embedded().unwrap();
        apply(&mut connection, &broken).unwrap();
        broken.push(Migration {
            version: 3,
            sql: "CREATE TABLE partial (id TEXT); THIS IS NOT SQL;",
        });

        assert!(apply(&mut connection, &broken).is_err());
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            2
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
        connection.pragma_update(None, "user_version", 3).unwrap();

        let error = apply(&mut connection, &embedded().unwrap()).unwrap_err();

        assert!(matches!(
            error,
            DatabaseError::NewerSchema {
                found: 3,
                supported: 2
            }
        ));
    }
}
