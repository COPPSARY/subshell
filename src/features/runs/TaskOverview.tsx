import { Check, Circle, CircleX, LoaderCircle, TerminalSquare } from "lucide-react";
import type { Task } from "../tasks";
import { ContextSharePanel } from "../context/ContextSharePanel";
import type { Run, TaskPlan } from "./model";
import { TaskPlanPanel } from "./TaskPlanPanel";

export function TaskOverview({ onApprovePlan, onRejectPlan, onSelectRun, plan, planBusy = false, runs, task }: { onApprovePlan?: (fullAccess: boolean) => void; onRejectPlan?: () => void; onSelectRun: (id: string) => void; plan?: TaskPlan; planBusy?: boolean; runs: Run[]; task: Task }) {
  const active = runs.find((run) => ["queued", "preparing", "running", "waiting"].includes(run.status)) ?? runs[0];
  const failed = runs.some((run) => ["failed", "cancelled"].includes(run.status));
  const succeeded = runs.length > 0 && runs.every((run) => run.status === "succeeded");
  const steps = [
    { label: "Task created", state: "complete" },
    { label: "Agent workspace prepared", state: runs.length ? "complete" : "pending" },
    { label: "Implementation", state: failed ? "failed" : succeeded ? "complete" : active ? "active" : "pending" },
    { label: "Ready for review", state: ["review", "approved", "merged", "archived"].includes(task.status) || succeeded ? "complete" : "pending" },
  ] as const;
  return <div className="grid min-h-0 flex-1 overflow-auto bg-surface 2xl:grid-cols-[minmax(0,1fr)_18rem]" aria-label="Task overview">
    <div className="p-6">
      {plan && onApprovePlan && onRejectPlan && <TaskPlanPanel busy={planBusy} onApprove={onApprovePlan} onReject={onRejectPlan} plan={plan} />}
      <section className={plan ? "mt-6" : undefined}>
        <p className="text-[11px] font-semibold uppercase tracking-wider text-tertiary">Implementation</p>
        {active ? <><h2 className="mt-3 text-xl font-medium text-primary">{agentLabel(active, runs.indexOf(active))} is {active.status}</h2><p className="mt-2 max-w-3xl whitespace-pre-wrap text-sm leading-6 text-secondary">{active.instruction}</p><button className="button-primary mt-4" onClick={() => onSelectRun(active.id)} type="button"><TerminalSquare size={14} />Open live terminal</button></> : <><h2 className="mt-3 text-xl font-medium text-primary">Ready to start</h2><p className="mt-2 text-sm text-secondary">Add an agent session to begin work on this task.</p></>}
      </section>

      <section className="mt-8">
        <h3 className="text-sm font-medium text-primary">Progress</h3>
        <ol aria-label="Task progress" className="mt-3 divide-y divide-line border-y border-line">{steps.map((step) => <li className="flex min-h-10 items-center gap-3 py-2 text-sm" key={step.label}>{step.state === "complete" ? <Check aria-hidden="true" className="text-complete" size={14} /> : step.state === "active" ? <LoaderCircle aria-hidden="true" className="text-accent" size={14} /> : step.state === "failed" ? <CircleX aria-hidden="true" className="text-failed" size={14} /> : <Circle aria-hidden="true" className="text-tertiary" size={14} />}<span className={step.state === "pending" ? "text-tertiary" : "text-primary"}>{step.label}</span><span className="ml-auto text-[10px] uppercase text-tertiary">{step.state}</span></li>)}</ol>
      </section>
      <ContextSharePanel runs={runs} />

      <section className="mt-8">
        <h3 className="text-sm font-medium text-primary">Agent activity</h3>
        {runs.length ? <ul className="mt-3 divide-y divide-line border-y border-line">{runs.map((run, index) => <li className="flex min-h-14 items-center gap-3 py-2" key={run.id}><span aria-hidden="true" className={`size-2 shrink-0 rounded-full ${statusDot(run.status)}`} /><span className="min-w-0 flex-1"><strong className="block truncate text-sm font-medium">{agentLabel(run, index)}</strong><span className="block truncate text-xs text-tertiary">{run.instruction}</span></span><span className="status-pill">{run.status}</span></li>)}</ul> : <p className="mt-2 text-sm text-tertiary">No agent activity yet.</p>}
      </section>

      <section className="mt-8">
        <h3 className="text-sm font-medium text-primary">Acceptance criteria</h3>
        {task.acceptanceCriteria.length ? <ul className="mt-3 divide-y divide-line border-y border-line">{task.acceptanceCriteria.map((criterion) => <li className="flex min-h-10 items-start gap-3 py-2 text-sm text-secondary" key={criterion}><Circle aria-hidden="true" className="mt-0.5 shrink-0 text-tertiary" size={14} /><span>{criterion}</span></li>)}</ul> : <p className="mt-2 text-sm text-tertiary">No acceptance criteria were supplied for this quick task.</p>}
      </section>
    </div>

    <aside className="hidden bg-chrome p-4 2xl:block" aria-label="Assigned agents">
      <div className="flex items-center justify-between"><h3 className="text-xs font-semibold uppercase tracking-wider text-secondary">Agents</h3><span className="font-mono text-[10px] text-tertiary">{runs.length}</span></div>
      <div className="mt-3 grid gap-1">{runs.map((run, index) => <button className="flex min-h-14 w-full items-center gap-3 rounded-md px-2 text-left hover:bg-panel" key={run.id} onClick={() => onSelectRun(run.id)} type="button"><span aria-hidden="true" className={`size-2 shrink-0 rounded-full ${statusDot(run.status)}`} /><span className="min-w-0 flex-1"><strong className="block truncate text-xs font-medium">{run.role === "planner" ? "Planner" : run.title || (index === 0 ? "Lead" : `Agent ${index + 1}`)}</strong><span className="block truncate text-[11px] text-tertiary">{run.providerName}</span></span><span className="text-[10px] uppercase text-secondary">{run.status}</span></button>)}</div>
    </aside>
  </div>;
}

export function agentLabel(run: Run, index: number) {
  const role = run.role === "planner" ? "Planner" : run.title || (index === 0 ? "Lead" : `Agent ${index + 1}`);
  return `${role} · ${run.providerName}`;
}

export function statusDot(status: string) {
  if (["queued", "preparing", "running", "working"].includes(status)) return "bg-accent";
  if (status === "waiting") return "bg-waiting";
  if (status === "succeeded") return "bg-complete";
  if (["failed", "cancelled"].includes(status)) return "bg-failed";
  return "bg-tertiary";
}
