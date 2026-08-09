import { useEffect, useState } from "react";
import { Archive, Check, ChevronDown, ChevronRight, CircleStop, CircleX, FolderGit2, GitBranch, MoreHorizontal, RotateCcw } from "lucide-react";
import type { Project } from "../features/projects";
import { ProviderIcon } from "../features/providers";
import { listRuns, type Run } from "../features/runs";
import { listArchivedTasks, listTasks, updateTaskStatus, type Task } from "../features/tasks";

type Workspace = { task: Task; runs: Run[] };
type Props = {
  project: Project | null;
  selectedRunId?: string | null;
  selectedTaskId?: string | null;
  onSelect: (task: Task, runId?: string) => void;
  onArchived?: (taskId: string) => void;
};

const terminalTaskStatuses = new Set(["review", "approved", "merged", "failed", "cancelled"]);

export function WorkspaceRail({ project, selectedRunId, selectedTaskId, onSelect, onArchived }: Props) {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [archivedTasks, setArchivedTasks] = useState<Task[]>([]);
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [menuTaskId, setMenuTaskId] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState("");

  useEffect(() => {
    let mounted = true;
    if (!project) { setWorkspaces([]); setArchivedTasks([]); return; }
    // ponytail: polling is sufficient for local task counts; switch to backend events if it becomes noisy.
    const load = async () => {
      try {
        const [tasks, archived] = await Promise.all([listTasks(project.id), listArchivedTasks(project.id)]);
        const items = await Promise.all(tasks.map(async (task) => ({ task, runs: await listRuns(task.id) })));
        if (mounted) { setWorkspaces(items); setArchivedTasks(archived); setError(""); }
      } catch {
        if (mounted) setError("Could not load workspaces");
      }
    };
    load();
    const timer = window.setInterval(load, 4000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [project?.id, reload]);

  useEffect(() => {
    if (!menuTaskId) return;
    const close = () => setMenuTaskId(null);
    const keydown = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    window.addEventListener("click", close);
    window.addEventListener("keydown", keydown);
    return () => { window.removeEventListener("click", close); window.removeEventListener("keydown", keydown); };
  }, [menuTaskId]);

  async function archiveTask(task: Task) {
    try {
      await updateTaskStatus(task.id, "archived");
      setMenuTaskId(null);
      setReload((value) => value + 1);
      onArchived?.(task.id);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }

  async function restoreTask(task: Task) {
    try {
      const restored = await updateTaskStatus(task.id, "task");
      setReload((value) => value + 1);
      if (selectedTaskId === task.id) onSelect(restored);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }

  const activeAgents = workspaces.flatMap((workspace) => workspace.runs).filter((run) => ["queued", "preparing", "running"].includes(run.status)).length;

  return <section className="flex min-h-0 flex-1 flex-col px-2.5 py-3" aria-label="Workspaces">
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mb-2 flex items-center justify-between px-1.5">
        <h2 className="text-[11px] font-bold uppercase tracking-[0.12em] text-tertiary">Workspaces</h2>
        <span className="flex items-center gap-1.5 font-mono text-[10px] text-tertiary"><span aria-hidden="true" className={`size-1.5 rounded-full ${activeAgents ? "bg-accent" : "bg-tertiary"}`} />{activeAgents ? `${activeAgents} active` : "Idle"}</span>
      </div>

      {!project ? <div className="px-2 py-3"><FolderGit2 aria-hidden="true" className="mb-2 text-tertiary" size={15} /><p className="text-xs font-medium text-secondary">No project open</p><p className="mt-1 text-[11px] leading-4 text-tertiary">Open a folder to start a workspace.</p></div> : <div className="px-2 py-2"><div className="flex items-center gap-2"><FolderGit2 aria-hidden="true" className="shrink-0 text-secondary" size={14} /><span className="min-w-0 flex-1 truncate text-xs font-medium text-primary">{project.name}</span><span className="max-w-20 truncate font-mono text-[9px] text-tertiary">{project.git.branch ?? "detached"}</span></div><p className="mt-1 truncate pl-[1.375rem] font-mono text-[9px] text-tertiary">{project.path}</p></div>}

      {error && <p className="px-2 py-2 text-xs text-failed" role="alert">{error}</p>}
      {project && !error && !workspaces.length ? <div className="px-2 py-4"><p className="text-xs font-medium text-secondary">No active workspaces</p><p className="mt-1 text-[11px] leading-4 text-tertiary">Start a goal or restore one from Archive.</p></div> : project && <div className="ml-3 border-l border-line/70 pl-2">
        {workspaces.map(({ task, runs }) => {
          const canArchive = terminalTaskStatuses.has(task.status) && runs.every((run) => ["succeeded", "failed", "cancelled"].includes(run.status));
          const taskSelected = task.id === selectedTaskId && !selectedRunId;
          return <div className="relative mb-0.5" key={task.id}>
            <span aria-hidden="true" className={`absolute -left-2 top-4 h-px w-2 ${taskSelected ? "bg-accent" : "bg-line/70"}`} />
            <div className="group relative flex items-center" onContextMenu={(event) => { if (!canArchive) return; event.preventDefault(); setMenuTaskId(task.id); }}>
              <button aria-current={taskSelected ? "page" : undefined} className="group/task flex h-9 min-w-0 flex-1 items-center gap-2 px-2 text-left text-xs text-secondary outline-none hover:text-primary focus-visible:ring-1 focus-visible:ring-line-strong aria-[current=page]:text-primary" onClick={() => onSelect(task)} type="button">
                <GitBranch aria-hidden="true" className="shrink-0 text-tertiary group-hover/task:text-secondary group-aria-[current=page]/task:text-accent" size={13} />
                <span className="min-w-0 flex-1 truncate font-medium">{task.title}</span>
                <StateIndicator status={task.status} />
              </button>
              {canArchive && <button aria-label={`Workspace actions for ${task.title}`} aria-expanded={menuTaskId === task.id} className="absolute right-5 flex size-7 items-center justify-center text-tertiary opacity-0 outline-none hover:text-primary focus:opacity-100 focus-visible:ring-1 focus-visible:ring-line-strong group-hover:opacity-100" onClick={(event) => { event.stopPropagation(); setMenuTaskId((current) => current === task.id ? null : task.id); }} type="button"><MoreHorizontal aria-hidden="true" size={13} /></button>}
              {menuTaskId === task.id && <div className="absolute right-1 top-8 z-20 min-w-36 border border-line-strong bg-panel p-1" role="menu"><button className="flex h-8 w-full items-center gap-2 px-2 text-left text-[11px] text-secondary outline-none hover:bg-selected hover:text-primary focus-visible:bg-selected focus-visible:text-primary" onClick={() => archiveTask(task)} role="menuitem" type="button"><Archive aria-hidden="true" size={13} />Archive workspace</button></div>}
            </div>

            {runs.length > 0 && <div className="ml-4 border-l border-line/70 pl-2">{runs.map((run, index) => {
              const runSelected = run.id === selectedRunId;
              return <div className="relative" key={run.id} onContextMenu={(event) => { if (!canArchive) return; event.preventDefault(); setMenuTaskId(task.id); }}><span aria-hidden="true" className={`absolute -left-2 top-4 h-px w-2 ${runSelected ? "bg-accent" : "bg-line/70"}`} /><button aria-current={runSelected ? "page" : undefined} className="flex h-8 w-full items-center gap-2 rounded-sm px-2 text-left text-[11px] text-tertiary outline-none hover:text-primary focus-visible:ring-1 focus-visible:ring-line-strong aria-[current=page]:bg-surface/70 aria-[current=page]:text-primary" onClick={() => onSelect(task, run.id)} type="button"><ProviderIcon aria-hidden="true" className="text-secondary" name={run.providerName} size={12} /><span className="min-w-0 flex-1 truncate"><strong className="font-medium text-secondary">{runLabel(run, index)}</strong><span className="text-tertiary"> · {run.providerName}</span></span><StateIndicator status={run.status} /></button></div>;
            })}</div>}
          </div>;
        })}
      </div>}
    </div>

    <div className="mt-2 shrink-0 border-t border-line pt-2">
      <button aria-expanded={archiveOpen} className="flex h-8 w-full items-center gap-2 px-2 text-left text-[11px] text-tertiary outline-none hover:text-primary focus-visible:ring-1 focus-visible:ring-line-strong" onClick={() => setArchiveOpen((value) => !value)} type="button"><Archive aria-hidden="true" size={13} /><span className="flex-1">Archive</span><span className="font-mono text-[9px]">{archivedTasks.length}</span>{archiveOpen ? <ChevronDown aria-hidden="true" size={12} /> : <ChevronRight aria-hidden="true" size={12} />}</button>
      {archiveOpen && <div className="max-h-48 overflow-y-auto pt-1">{archivedTasks.length ? archivedTasks.map((task) => <div className="group flex h-8 items-center" key={task.id}><button className="min-w-0 flex-1 truncate px-2 text-left text-[11px] text-tertiary outline-none hover:text-primary focus-visible:ring-1 focus-visible:ring-line-strong" onClick={() => onSelect(task)} type="button">{task.title}</button><button aria-label={`Restore ${task.title}`} className="flex size-7 shrink-0 items-center justify-center text-tertiary opacity-0 outline-none hover:text-primary focus:opacity-100 focus-visible:ring-1 focus-visible:ring-line-strong group-hover:opacity-100" onClick={() => restoreTask(task)} title="Restore workspace" type="button"><RotateCcw aria-hidden="true" size={12} /></button></div>) : <p className="px-2 py-2 text-[10px] text-tertiary">No archived workspaces</p>}</div>}
    </div>
  </section>;
}

function runLabel(run: Run, index: number) {
  return run.role === "planner" ? "Planner" : run.title || (index === 0 ? "Lead" : `Agent ${index + 1}`);
}

function StateIndicator({ status }: { status: string }) {
  if (["succeeded", "review", "approved", "merged", "archived"].includes(status)) return <span title={status}><Check aria-hidden="true" className="text-complete" size={12} /><span className="sr-only">{status}</span></span>;
  if (status === "cancelled") return <span title="stopped"><CircleStop aria-hidden="true" className="text-tertiary" size={12} /><span className="sr-only">stopped</span></span>;
  if (status === "failed") return <span title="failed"><CircleX aria-hidden="true" className="text-failed" size={12} /><span className="sr-only">failed</span></span>;
  const color = status === "waiting" ? "bg-waiting" : ["working", "queued", "preparing", "running"].includes(status) ? "bg-accent" : "bg-tertiary";
  return <span title={status}><span aria-hidden="true" className={`block size-1.5 rounded-full ${color}`} /><span className="sr-only">{status}</span></span>;
}

function errorMessage(error: unknown) {
  return error && typeof error === "object" && "message" in error ? String(error.message) : String(error);
}
