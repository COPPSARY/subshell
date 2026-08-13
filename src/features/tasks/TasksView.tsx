import { useEffect, useState } from "react";
import { ArrowLeft, Circle, CircleAlert, CircleCheck, Eye, GripVertical, ListChecks, LoaderCircle, Plus, TriangleAlert } from "lucide-react";
import { errorMessage } from "../../shared/error";
import type { Project } from "../projects";
import { RunWorkspace } from "../runs";
import { createTask, getTask, listTasks, updateTaskStatus, type CreateTaskInput } from "./api";
import type { Task } from "./model";

type Props = { project?: Project | null; initialTask?: Task | null; autoStartTaskId?: string | null; initialRunId?: string | null; onAutoStartConsumed?: () => void; onSelectTask?: (task: Task | null) => void; onSelectRun?: (runId: string | null) => void };

const columns = [
  { name: "Backlog", moveStatus: "task", icon: Circle, statuses: ["task", "idea", "queued"], iconClass: "text-secondary" },
  { name: "Active", moveStatus: "working", icon: LoaderCircle, statuses: ["working", "preparing", "running", "waiting"], iconClass: "text-accent" },
  { name: "Review", moveStatus: "review", icon: Eye, statuses: ["review"], iconClass: "text-secondary" },
  { name: "Attention", moveStatus: "failed", icon: CircleAlert, statuses: ["failed", "cancelled"], iconClass: "text-failed" },
  { name: "Done", moveStatus: "approved", icon: CircleCheck, statuses: ["approved", "merged", "archived", "succeeded"], iconClass: "text-complete" },
] as const;
const knownStatuses = new Set<string>(columns.flatMap((column) => column.statuses));

export function TasksView({ project, initialTask, autoStartTaskId, initialRunId, onAutoStartConsumed, onSelectTask, onSelectRun }: Props) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [selected, setSelected] = useState<Task | null>(initialTask ?? null);
  const [creating, setCreating] = useState(false);
  const [loading, setLoading] = useState(Boolean(project));
  const [error, setError] = useState("");
  useEffect(() => {
    let mounted = true;
    setSelected(null);
    setError("");
    if (!project) { setTasks([]); setLoading(false); return; }
    setLoading(true);
    listTasks(project.id)
      .then((items) => { if (mounted) setTasks(items); })
      .catch((reason) => { if (mounted) setError(errorMessage(reason)); })
      .finally(() => { if (mounted) setLoading(false); });
    return () => { mounted = false; };
  }, [project?.id]);
  useEffect(() => { setSelected(initialTask && initialTask.projectId === project?.id ? initialTask : null); }, [initialTask?.id, project?.id]);
  useEffect(() => {
    if (!selected) return;
    let mounted = true;
    const refresh = () => getTask(selected.id).then((next) => { if (mounted) { setSelected(next); setTasks((items) => items.map((item) => item.id === next.id ? next : item)); onSelectTask?.(next); } }).catch(() => undefined);
    const timer = window.setInterval(refresh, 2000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [selected?.id]);
  async function moveTask(task: Task, status: string) {
    setError("");
    try {
      const updated = await updateTaskStatus(task.id, status);
      setTasks((items) => items.map((item) => item.id === updated.id ? updated : item));
      if (selected?.id === updated.id) { setSelected(updated); onSelectTask?.(updated); }
    } catch (reason) { setError(errorMessage(reason)); }
  }
  if (!project) return <div className="w-full p-7"><h1 className="text-[15px] font-medium">No tasks yet</h1><p className="mt-2 text-sm text-secondary">Open a project folder before creating a coordinated task.</p></div>;
  if (selected) return <div className="flex h-full w-full flex-col overflow-hidden px-4 pb-3"><header className="flex h-12 shrink-0 items-center gap-3 border-b border-line"><button className="icon-button" aria-label="Back to tasks" onClick={() => { setSelected(null); onSelectTask?.(null); }} type="button"><ArrowLeft size={14} /></button><div className="min-w-0"><h1 className="truncate text-[15px] font-medium">{selected.title}</h1><p className="mt-0.5 truncate text-[11px] text-tertiary">{selected.description && selected.description !== selected.title ? selected.description : `${project.name} · ${selected.baseBranch}`}</p></div><span className="status-pill ml-auto"><span aria-hidden="true" className={`size-2 rounded-full ${taskStatusDot(selected.status)}`} />{taskStage(selected.status)}</span></header>{error && <p className="error-banner my-2" role="alert">{error}</p>}<RunWorkspace autoStart={selected.id === autoStartTaskId} initialRunId={initialRunId} onActiveRunChange={onSelectRun} onAutoStartConsumed={onAutoStartConsumed} project={project} task={selected} /></div>;
  return <div className="min-h-full w-full p-5"><header className="flex min-h-14 items-center justify-between border-b border-line"><div><h1 className="text-base font-medium">Tasks</h1><p className="mt-1 text-xs text-tertiary">{project.name} · {tasks.length} workspace{tasks.length === 1 ? "" : "s"}</p></div><button className="button-primary" onClick={() => setCreating(true)} type="button"><Plus size={14} />New task</button></header>
    {error && <p className="error-banner" role="alert">{error}</p>}{project.git.dirty && <p className="flex min-h-10 items-center gap-2 border-b border-line px-2 text-xs text-secondary"><TriangleAlert aria-hidden="true" className="shrink-0 text-[#cabd8a]" size={14} />New tasks use committed HEAD; uncommitted changes stay in this checkout.</p>}
    {creating && <TaskForm dirty={project.git.dirty} onCancel={() => setCreating(false)} onCreate={async (values) => { try { const task = await createTask({ ...values, projectId: project.id }); setTasks((items) => [task, ...items]); setSelected(task); onSelectTask?.(task); setCreating(false); } catch (reason) { setError(errorMessage(reason)); } }} />}
    {loading ? <p className="empty-row" role="status">Loading tasks…</p> : !tasks.length && !creating ? <p className="empty-row">No tasks yet. Create one from a goal to start an isolated agent workspace.</p> : <TaskBoard onMove={moveTask} tasks={tasks} onSelect={(task) => { setSelected(task); onSelectTask?.(task); }} />}
  </div>;
}

function TaskBoard({ tasks, onMove, onSelect }: { tasks: Task[]; onMove: (task: Task, status: string) => void; onSelect: (task: Task) => void }) {
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [target, setTarget] = useState<string | null>(null);
  function moveByKeyboard(task: Task, direction: -1 | 1) {
    const current = columns.findIndex((column) => column.statuses.some((status) => status === task.status));
    const next = columns[current + direction];
    if (next) onMove(task, next.moveStatus);
  }
  return <div className="mt-4 grid items-start gap-3 md:grid-cols-2 xl:grid-cols-5" aria-label="Task board">
    {columns.map((column) => {
      const items = tasks.filter((task) => column.statuses.some((status) => status === task.status) || (column.name === "Backlog" && !knownStatuses.has(task.status)));
      const Icon = column.icon;
      return <section aria-label={`${column.name} tasks`} className={`min-w-0 border-t bg-chrome transition-colors ${target === column.name ? "border-accent bg-selected" : "border-line"}`} key={column.name} onDragEnter={() => setTarget(column.name)} onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = "move"; }} onDrop={(event) => { event.preventDefault(); const task = tasks.find((item) => item.id === event.dataTransfer.getData("text/plain")); if (task && !column.statuses.some((status) => status === task.status)) onMove(task, column.moveStatus); setDraggingId(null); setTarget(null); }}>
        <header className="flex h-10 items-center gap-2 border-b border-line px-3"><Icon aria-hidden="true" className={column.iconClass} size={13} /><h2 className="text-xs font-medium">{column.name}</h2><span className="ml-auto font-mono text-[10px] text-tertiary">{items.length}</span></header>
        <div className="min-h-24 p-2">{items.length ? items.map((task) => <article className={`mb-2 cursor-grab rounded-md border border-line bg-surface p-3 hover:border-line-strong active:cursor-grabbing ${draggingId === task.id ? "opacity-40" : ""}`} draggable key={task.id} onDragEnd={() => { setDraggingId(null); setTarget(null); }} onDragStart={(event) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", task.id); setDraggingId(task.id); }}><button className="block w-full text-left" onClick={() => onSelect(task)} onKeyDown={(event) => { if (!event.altKey || !["ArrowLeft", "ArrowRight"].includes(event.key)) return; event.preventDefault(); moveByKeyboard(task, event.key === "ArrowLeft" ? -1 : 1); }} title="Open task. Drag to another column to move it; use Alt+Left or Alt+Right from the keyboard." type="button"><strong className="line-clamp-2 text-sm font-medium leading-5">{task.title}</strong>{task.description && task.description !== task.title && <span className="mt-1 block line-clamp-2 text-xs leading-4 text-secondary">{task.description}</span>}</button><footer className="mt-3 flex items-center gap-2"><GripVertical aria-hidden="true" className="text-tertiary" size={13} /><span aria-hidden="true" className={`size-2 rounded-full ${taskStatusDot(task.status)}`} /><span className="text-[11px] text-secondary">{taskStage(task.status)}</span>{task.acceptanceCriteria.length > 0 && <span className="ml-auto flex items-center gap-1 text-[10px] text-tertiary"><ListChecks aria-hidden="true" size={12} />{task.acceptanceCriteria.length}</span>}<span className="font-mono text-[10px] text-tertiary">{task.baseRevision.slice(0, 8)}</span></footer></article>) : <p className="px-1 py-4 text-[11px] text-tertiary">{draggingId ? "Drop task here" : "No tasks"}</p>}</div>
      </section>;
    })}
  </div>;
}

function taskStage(status: string) { if (status === "cancelled") return "Stopped"; return columns.find((column) => column.statuses.some((value) => value === status))?.name ?? "Backlog"; }
function taskStatusDot(status: string) { if (["working", "queued"].includes(status)) return "bg-accent"; if (status === "waiting") return "bg-waiting"; if (["review", "approved", "merged", "archived"].includes(status)) return "bg-complete"; if (status === "failed") return "bg-failed"; return "bg-tertiary"; }

function TaskForm({ dirty, onCancel, onCreate }: { dirty: boolean; onCancel: () => void; onCreate: (input: Omit<CreateTaskInput, "projectId">) => void }) {
  const [title, setTitle] = useState(""); const [description, setDescription] = useState(""); const [criteria, setCriteria] = useState(""); const [paths, setPaths] = useState(""); const [commands, setCommands] = useState(""); const [decisions, setDecisions] = useState(""); const [confirm, setConfirm] = useState(false);
  return <form className="form-panel my-4" onSubmit={(event) => { event.preventDefault(); onCreate({ title, description, acceptanceCriteria: lines(criteria), allowedPaths: lines(paths), validationCommands: lines(commands), decisions: lines(decisions), confirmDirtyBase: confirm }); }}><label>Title<input required value={title} onChange={(event) => setTitle(event.target.value)} /></label><label>Description<textarea rows={3} value={description} onChange={(event) => setDescription(event.target.value)} /></label><div className="grid gap-3 lg:grid-cols-2"><label>Acceptance criteria<textarea rows={3} placeholder="One item per line" value={criteria} onChange={(event) => setCriteria(event.target.value)} /></label><label>Allowed paths<textarea rows={3} placeholder="src/features/example/**" value={paths} onChange={(event) => setPaths(event.target.value)} /></label><label>Validation commands<textarea rows={3} placeholder="npm test" value={commands} onChange={(event) => setCommands(event.target.value)} /></label><label>Decisions<textarea rows={3} placeholder="One decision per line" value={decisions} onChange={(event) => setDecisions(event.target.value)} /></label></div>{dirty && <label className="check-row"><input checked={confirm} onChange={(event) => setConfirm(event.target.checked)} required type="checkbox" />Use committed HEAD and exclude uncommitted changes</label>}<div className="flex gap-2"><button className="button-primary" type="submit">Create task</button><button className="button-secondary" onClick={onCancel} type="button">Cancel</button></div></form>;
}
function lines(value: string) { return value.split("\n").map((line) => line.trim()).filter(Boolean); }
