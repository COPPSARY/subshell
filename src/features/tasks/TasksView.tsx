import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import type { Project } from "../projects";
import { RunWorkspace } from "../runs";
import { createTask, listTasks, type CreateTaskInput } from "./api";
import type { Task } from "./model";

export function TasksView({ project }: { project?: Project | null }) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [selected, setSelected] = useState<Task | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => { setSelected(null); if (project) listTasks(project.id).then(setTasks).catch((reason) => setError(String(reason))); else setTasks([]); }, [project]);
  if (!project) return <div className="w-full p-7"><h1 className="text-[15px] font-medium">No tasks yet</h1><p className="mt-2 text-sm text-secondary">Open a Git repository before creating a coordinated task.</p></div>;
  return <div className="w-full p-7"><div className="flex h-11 items-center justify-between border-b border-line"><h1 className="text-[15px] font-medium">Tasks · {project.name}</h1><button className="button-primary" onClick={() => setCreating(true)} type="button"><Plus size={14} />New task</button></div>
    {error && <p className="error-banner" role="alert">{error}</p>}{project.git.dirty && <p className="warning-banner">The checkout is dirty. New tasks capture committed HEAD and exclude local changes.</p>}
    {creating && <TaskForm dirty={project.git.dirty} onCancel={() => setCreating(false)} onCreate={async (values) => { try { const task = await createTask({ ...values, projectId: project.id }); setTasks((items) => [task, ...items]); setSelected(task); setCreating(false); } catch (reason) { setError(String(reason)); } }} />}
    {!tasks.length && !creating ? <p className="empty-row">No tasks yet</p> : <div className="divide-y divide-line border-b border-line">{tasks.map((task) => <button className="flex min-h-14 w-full items-center gap-3 px-3 text-left hover:bg-panel" key={task.id} onClick={() => setSelected(task)} type="button"><strong className="flex-1 text-sm font-medium">{task.title}</strong><span className="status-pill">{task.status}</span><span className="font-mono text-[11px] text-tertiary">{task.baseRevision.slice(0, 8)}</span></button>)}</div>}
    {selected && <RunWorkspace project={project} task={selected} />}
  </div>;
}

function TaskForm({ dirty, onCancel, onCreate }: { dirty: boolean; onCancel: () => void; onCreate: (input: Omit<CreateTaskInput, "projectId">) => void }) {
  const [title, setTitle] = useState(""); const [description, setDescription] = useState(""); const [criteria, setCriteria] = useState(""); const [paths, setPaths] = useState(""); const [commands, setCommands] = useState(""); const [decisions, setDecisions] = useState(""); const [confirm, setConfirm] = useState(false);
  return <form className="form-panel my-4" onSubmit={(event) => { event.preventDefault(); onCreate({ title, description, acceptanceCriteria: lines(criteria), allowedPaths: lines(paths), validationCommands: lines(commands), decisions: lines(decisions), confirmDirtyBase: confirm }); }}><label>Title<input required value={title} onChange={(event) => setTitle(event.target.value)} /></label><label>Description<textarea rows={3} value={description} onChange={(event) => setDescription(event.target.value)} /></label><div className="grid gap-3 lg:grid-cols-2"><label>Acceptance criteria<textarea rows={3} placeholder="One item per line" value={criteria} onChange={(event) => setCriteria(event.target.value)} /></label><label>Allowed paths<textarea rows={3} placeholder="src/features/example/**" value={paths} onChange={(event) => setPaths(event.target.value)} /></label><label>Validation commands<textarea rows={3} placeholder="npm test" value={commands} onChange={(event) => setCommands(event.target.value)} /></label><label>Decisions<textarea rows={3} placeholder="One decision per line" value={decisions} onChange={(event) => setDecisions(event.target.value)} /></label></div>{dirty && <label className="check-row"><input checked={confirm} onChange={(event) => setConfirm(event.target.checked)} required type="checkbox" />Use committed HEAD and exclude uncommitted changes</label>}<div className="flex gap-2"><button className="button-primary" type="submit">Create task</button><button className="button-secondary" onClick={onCancel} type="button">Cancel</button></div></form>;
}
function lines(value: string) { return value.split("\n").map((line) => line.trim()).filter(Boolean); }
