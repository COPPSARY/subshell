import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderGit2, Plus } from "lucide-react";
import { listProjects, openProject } from "./api";
import type { Project } from "./model";

export function ProjectsView({ onSelect }: { onSelect?: (project: Project) => void }) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);

  const load = () => listProjects().then(setProjects).catch((reason) => setError(message(reason))).finally(() => setLoading(false));
  useEffect(() => { void load(); }, []);

  async function chooseProject() {
    const path = await open({ directory: true, multiple: false, title: "Open project" });
    if (!path) return;
    setError("");
    try {
      const project = await openProject(path);
      setProjects((current) => [project, ...current.filter((item) => item.id !== project.id)]);
      onSelect?.(project);
    } catch (reason) {
      setError(message(reason));
    }
  }

  return (
    <div className="w-full p-7">
      <div className="flex h-11 items-center justify-between border-b border-line">
        <h1 className="text-[15px] font-medium text-primary">Recent repositories</h1>
        <button className="button-primary" onClick={chooseProject} type="button"><Plus size={14} />Open repository</button>
      </div>
      {error && <p className="error-banner" role="alert">{error}</p>}
      <div className="grid grid-cols-[minmax(220px,1fr)_180px_140px] border-b border-line px-3 py-2.5 table-label"><span>Repository</span><span>Branch</span><span>State</span></div>
      {loading ? <p className="empty-row">Loading repositories…</p> : projects.length ? projects.map((project) => (
        <button className="grid min-h-14 w-full grid-cols-[minmax(220px,1fr)_180px_140px] items-center border-b border-line px-3 text-left text-sm hover:bg-panel" key={project.id} onClick={() => onSelect?.(project)} type="button">
          <span><strong className="block font-medium text-primary">{project.name}</strong><small className="text-tertiary">{project.path}</small></span>
          <span className="font-mono text-xs text-secondary">{project.git.branch ?? "detached"}</span>
          <span className="text-xs text-secondary">{project.git.isRepository ? project.git.dirty ? "Dirty" : "Clean" : "Not Git"}</span>
        </button>
      )) : <div className="empty-row flex items-center gap-4"><span className="icon-box"><FolderGit2 size={17} /></span><span><h2 className="text-sm font-medium text-primary">No repositories yet</h2>Open a local Git repository to coordinate agent runs.</span></div>}
    </div>
  );
}

function message(error: unknown) { return typeof error === "string" ? error : error instanceof Error ? error.message : "The project could not be opened."; }
