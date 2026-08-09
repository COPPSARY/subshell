import { useEffect, useState } from "react";
import { Bot, Plus } from "lucide-react";
import type { Project } from "../projects";
import { ProviderIcon } from "../providers";
import type { Task } from "../tasks";
import { listRuns } from "./api";
import type { Run } from "./model";

type Agent = { run: Run; task: Task };
type Props = { project: Project | null; tasks: Task[]; onNewGoal: () => void; onOpen: (task: Task, runId: string) => void };
const activeStatuses = new Set(["queued", "preparing", "running", "waiting"]);

export function AgentsView({ project, tasks, onNewGoal, onOpen }: Props) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [loading, setLoading] = useState(Boolean(project));

  useEffect(() => {
    let mounted = true;
    if (!project) { setAgents([]); setLoading(false); return; }
    const load = () => Promise.all(tasks.map(async (task) => (await listRuns(task.id)).map((run) => ({ run, task })))).then((groups) => {
      if (mounted) { setAgents(groups.flat().filter(({ run }) => activeStatuses.has(run.status))); setLoading(false); }
    }).catch(() => mounted && setLoading(false));
    load();
    const timer = window.setInterval(load, 3000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [project?.id, tasks]);

  return <div className="min-h-full w-full p-5">
    <header className="flex min-h-14 items-center justify-between border-b border-line"><div><h1 className="text-base font-medium">Agents</h1><p className="mt-1 text-xs text-tertiary">All live coding sessions in {project?.name ?? "your workspace"}</p></div><button className="button-primary" disabled={!project} onClick={onNewGoal} type="button"><Plus aria-hidden="true" size={14} />New goal</button></header>
    {!project ? <p className="empty-row">Open a project to coordinate agents.</p> : loading ? <p className="empty-row" role="status">Loading agents…</p> : agents.length ? <ul className="divide-y divide-line" aria-label="Active agents">{agents.map(({ run, task }, index) => <li key={run.id}><button className="flex min-h-16 w-full items-center gap-3 px-2 text-left hover:bg-surface/70 focus-visible:outline focus-visible:outline-1 focus-visible:outline-line-strong" onClick={() => onOpen(task, run.id)} type="button"><ProviderIcon aria-hidden="true" className="text-secondary" name={run.providerName} size={16} /><span className="min-w-0 flex-1"><strong className="block truncate text-sm font-medium text-primary">{run.role === "planner" ? "Planner" : run.title || (index === 0 ? "Lead" : `Agent ${index + 1}`)} · {run.providerName}</strong><span className="mt-0.5 block truncate text-xs text-tertiary">{task.title}</span></span><span className="flex shrink-0 items-center gap-1.5 text-[10px] uppercase text-secondary"><span aria-hidden="true" className={`size-1.5 rounded-full ${run.status === "waiting" ? "bg-waiting" : "bg-accent"}`} />{run.status}</span></button></li>)}</ul> : <div className="grid min-h-64 place-items-center text-center"><div><Bot aria-hidden="true" className="mx-auto text-tertiary" size={22} /><h2 className="mt-3 text-sm font-medium">No agents working</h2><p className="mt-1 text-xs text-tertiary">Start one goal; the planner coordinates the sessions behind it.</p></div></div>}
  </div>;
}
