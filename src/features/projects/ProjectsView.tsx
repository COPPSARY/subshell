import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { File, FolderOpen, GitBranch, Play } from "lucide-react";
import { getProjectStatus, listProjectFiles, listProjects, openProject } from "./api";
import type { Project } from "./model";

type Props = {
  selectedProject?: Project | null;
  onSelect?: (project: Project) => void;
  onStartGoal?: (project: Project, goal: string, confirmDirtyBase: boolean) => Promise<void>;
};

export function ProjectsView({ selectedProject, onSelect, onStartGoal }: Props) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [selected, setSelected] = useState<Project | null>(selectedProject ?? null);
  const [files, setFiles] = useState<string[]>([]);
  const [fileTotal, setFileTotal] = useState(0);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [filesLoading, setFilesLoading] = useState(false);
  const [goal, setGoal] = useState("");
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    listProjects()
      .then((items) => { setProjects(items); setSelected((current) => current ?? selectedProject ?? items[0] ?? null); })
      .catch((reason) => setError(message(reason)))
      .finally(() => setLoading(false));
  }, []);
  useEffect(() => { if (selectedProject) setSelected(selectedProject); }, [selectedProject?.id]);
  useEffect(() => {
    if (!selected) return;
    let cancelled = false;
    const refresh = () => getProjectStatus(selected.path).then((git) => {
      if (cancelled || sameStatus(git, selected.git)) return;
      const updated = { ...selected, git };
      setSelected(updated);
      onSelect?.(updated);
    }).catch(() => undefined);
    const timer = window.setInterval(refresh, 2000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [selected?.id, selected?.path, selected?.git.isRepository, selected?.git.branch, selected?.git.revision, selected?.git.dirty]);
  useEffect(() => {
    let cancelled = false;
    if (!selected?.git.isRepository) { setFiles([]); setFileTotal(0); return; }
    setFilesLoading(true);
    listProjectFiles(selected.id).then((result) => { if (!cancelled) { setFiles(result.items); setFileTotal(result.total); } }).catch((reason) => !cancelled && setError(message(reason))).finally(() => !cancelled && setFilesLoading(false));
    return () => { cancelled = true; };
  }, [selected?.id, selected?.git.isRepository]);

  function select(project: Project) { setSelected(project); onSelect?.(project); }
  async function startGoal(event: React.FormEvent) {
    event.preventDefault();
    if (!selected || !onStartGoal) return;
    setError(""); setStarting(true);
    try { await onStartGoal(selected, goal.trim(), selected.git.dirty); }
    catch (reason) { setError(message(reason)); }
    finally { setStarting(false); }
  }
  async function chooseProject() {
    const path = await open({ directory: true, multiple: false, title: "Open project" });
    if (!path) return;
    setError("");
    try {
      const project = await openProject(path);
      setProjects((current) => [project, ...current.filter((item) => item.id !== project.id)]);
      select(project);
    } catch (reason) { setError(message(reason)); }
  }

  return <div className="mx-auto min-h-full w-full max-w-6xl p-6 xl:p-8">
    <div className="flex h-11 items-center justify-between border-b border-line"><div><h1 className="text-[15px] font-medium text-primary">Projects</h1>{selected && <p className="mt-0.5 text-[11px] text-tertiary">Choose a goal and SubShell prepares the agent workspace.</p>}</div>{selected && <button className="button-primary" onClick={chooseProject} type="button"><FolderOpen size={14} />Open project</button>}</div>
    {error && <p className="error-banner" role="alert">{error}</p>}
    {loading ? <p className="empty-row">Loading projects…</p> : !selected ? <div className="grid min-h-[calc(100vh-11rem)] place-items-center"><div className="max-w-md text-center"><span className="icon-box mx-auto mb-4"><FolderOpen size={18} /></span><h2 className="text-xl font-medium text-primary">Open your first project</h2><p className="mt-2 text-sm leading-6 text-secondary">Choose a local Git folder. Then describe the work—you do not need to create tasks, branches, or agent profiles first.</p><button className="button-primary mx-auto mt-5" onClick={chooseProject} type="button"><FolderOpen size={14} />Choose folder</button></div></div> : <>
      <section className="pt-7">
        <div className="flex min-w-0 items-start gap-3"><span className="icon-box"><FolderOpen size={16} /></span><div className="min-w-0 flex-1"><h2 className="truncate text-lg font-medium text-primary">{selected.name}</h2><p className="mt-1 truncate font-mono text-[11px] text-tertiary">{selected.path}</p></div><span className="flex shrink-0 items-center gap-1.5 rounded-full bg-panel px-2.5 py-1 font-mono text-[10px] text-secondary"><GitBranch size={11} />{selected.git.branch ?? "detached"}</span></div>
        <div className="mt-7"><h3 className="text-base font-medium text-primary">What should your agent work on?</h3><p className="mt-1 text-sm text-secondary">Describe the outcome in plain language. Context, task setup, and an isolated worktree are automatic.</p></div>
        {selected.git.isRepository && selected.git.revision && onStartGoal ? <form className="mt-4 overflow-hidden rounded-lg border border-line-strong bg-surface text-left shadow-xl shadow-black/15 focus-within:border-[#555b66]" onSubmit={startGoal}>
          <label className="sr-only" htmlFor="project-goal">What do you want the agent to do?</label>
          <textarea autoFocus className="project-goal min-h-28 w-full resize-none bg-transparent p-4 text-sm leading-6 text-primary outline-none placeholder:text-tertiary" id="project-goal" onChange={(event) => setGoal(event.target.value)} placeholder="Build a feature, fix a bug, investigate a problem…" required value={goal} />
          {selected.git.dirty && <p className="mx-4 mb-3 text-xs text-secondary">Runs use committed HEAD; local changes stay in this checkout.</p>}
          <div className="flex items-center justify-between border-t border-line bg-chrome px-3 py-2"><span className="text-xs text-tertiary">Uses your first ready coding CLI</span><button className="button-primary" disabled={starting || !goal.trim()} type="submit"><Play size={14} />{starting ? "Preparing workspace…" : "Start agent"}</button></div>
        </form> : <p className="warning-banner mt-6 text-left">{selected.git.isRepository ? "This repository has no commits yet. Create its first commit so SubShell has a safe worktree base." : "Git is required to create isolated agent worktrees in this project."}</p>}
      </section>

      <div className="mt-8 grid gap-8 lg:grid-cols-[minmax(0,1fr)_17rem]">
        <section><div className="flex items-center justify-between border-b border-line px-2 py-2"><h3 className="table-label">Project files</h3><span className="font-mono text-[10px] text-tertiary">{fileTotal}</span></div>{filesLoading ? <p className="empty-row">Reading project files…</p> : files.length ? <ul className="max-h-72 overflow-auto py-1" role="list">{files.map((path) => <li className="flex min-h-8 items-center gap-2 rounded px-2 font-mono text-[11px] text-secondary hover:bg-panel" key={path}><File aria-hidden="true" className="shrink-0 text-tertiary" size={13} /><span className="truncate">{path}</span></li>)}</ul> : <p className="empty-row">No project files found.</p>}</section>
        <section><h3 className="table-label border-b border-line px-2 py-2">Recent projects</h3>{projects.filter((project) => project.id !== selected.id).length ? projects.filter((project) => project.id !== selected.id).map((project) => <button className="flex min-h-11 w-full items-center gap-2 rounded px-2 text-left text-xs hover:bg-panel" key={project.id} onClick={() => select(project)} type="button"><FolderOpen className="shrink-0 text-tertiary" size={13} /><span className="min-w-0 flex-1 truncate font-medium text-primary">{project.name}</span><span className="font-mono text-[9px] text-tertiary">{project.git.branch ?? "detached"}</span></button>) : <p className="px-2 py-3 text-xs leading-5 text-tertiary">Other folders you open will appear here.</p>}</section>
      </div>
    </>}
  </div>;
}

function message(error: unknown) { if (typeof error === "string") return error; if (error && typeof error === "object" && "message" in error) return String(error.message); return "The project folder could not be read."; }
function sameStatus(left: Project["git"], right: Project["git"]) { return left.isRepository === right.isRepository && left.branch === right.branch && left.revision === right.revision && left.dirty === right.dirty; }
