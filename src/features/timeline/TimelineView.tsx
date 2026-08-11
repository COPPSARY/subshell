import { useEffect, useMemo, useRef, useState } from "react";
import { Activity, Bell, Bot, Check, CircleAlert, ExternalLink, GitPullRequest, ListTodo, MessageSquareText, X } from "lucide-react";
import { acknowledgeAttention, claimAttentionNotification, decideApproval, listAttention, listApprovals, type ApprovalRequest, type AttentionItem } from "../attention";
import type { Project } from "../projects";
import type { Task } from "../tasks";
import { listTimeline } from "./api";
import type { TimelineEvent } from "./model";

type Props = { project?: Project | null; tasks?: Task[]; onOpen?: (taskId: string, runId?: string | null) => void };

export function TimelineView({ project, tasks = [], onOpen }: Props) {
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [attention, setAttention] = useState<AttentionItem[]>([]);
  const [approvals, setApprovals] = useState<ApprovalRequest[]>([]);
  const [taskId, setTaskId] = useState("");
  const [kind, setKind] = useState("");
  const [scope, setScope] = useState<"global" | "project">("global");
  const [error, setError] = useState("");
  const [notificationPermission, setNotificationPermission] = useState<NotificationPermission>(() => typeof Notification === "undefined" ? "denied" : Notification.permission);
  const notified = useRef(new Set<string>());
  const pendingApprovals = useMemo(() => new Map(approvals.filter((item) => item.status === "pending").map((item) => [item.id, item])), [approvals]);
  const taskNames = useMemo(() => new Map(tasks.map((task) => [task.id, task.title])), [tasks]);
  useEffect(() => {
    let mounted = true;
    const load = () => Promise.all([listTimeline(scope === "global" ? null : project?.id ?? null, { taskId: taskId || null, eventType: kind || null }), project ? listAttention(project.id) : Promise.resolve([]), project ? listApprovals(project.id) : Promise.resolve([])])
      .then(([nextEvents, nextAttention, nextApprovals]) => { if (mounted) { setEvents(nextEvents); setAttention(nextAttention); setApprovals(nextApprovals); setError(""); notify(nextAttention, notified.current, onOpen); } })
      .catch((reason) => mounted && setError(errorMessage(reason)));
    load();
    const timer = window.setInterval(load, 2000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [project?.id, taskId, kind, scope]);
  async function acknowledge(item: AttentionItem) { await acknowledgeAttention(item.key, item.stateFingerprint); setAttention((items) => items.map((current) => current.key === item.key ? { ...current, acknowledged: true } : current)); }
  async function decide(requestId: string, decision: "approved" | "denied") { const updated = await decideApproval(requestId, decision); setApprovals((items) => items.map((item) => item.id === updated.id ? updated : item)); setAttention((items) => items.filter((item) => item.approvalRequestId !== requestId)); }
  return (
    <div className="h-full w-full overflow-auto p-5 lg:p-7">
      <h1 className="sr-only">Activity</h1>
      <div className="flex min-h-16 flex-wrap items-center justify-between gap-3 border-b border-line pb-3">
        <div><h2 className="text-base font-medium text-primary">Activity</h2><p className="mt-1 text-xs text-tertiary">{scope === "global" ? "Live history across projects" : `Live history for ${project?.name ?? "this project"}`}</p></div>
        <div className="flex flex-wrap items-center gap-2">{notificationPermission === "default" && <button className="button-secondary" onClick={() => Notification.requestPermission().then(setNotificationPermission)} type="button"><Bell size={13} />Enable alerts</button>}{project && <select aria-label="Activity scope" className="rounded-md border border-line bg-panel px-2 py-1.5 text-xs" onChange={(event) => { setScope(event.target.value as "global" | "project"); setTaskId(""); }} value={scope}><option value="global">All projects</option><option value="project">{project.name}</option></select>}<select aria-label="Filter by task" className="rounded-md border border-line bg-panel px-2 py-1.5 text-xs" onChange={(event) => setTaskId(event.target.value)} value={taskId}><option value="">All tasks</option>{scope === "project" && tasks.map((task) => <option key={task.id} value={task.id}>{task.title}</option>)}</select><select aria-label="Filter by event" className="rounded-md border border-line bg-panel px-2 py-1.5 text-xs" onChange={(event) => setKind(event.target.value)} value={kind}><option value="">All events</option><option value="task.status_changed">Task status</option><option value="run.status_changed">Run status</option><option value="context.shared">Context shared</option><option value="approval.requested">Approvals</option></select></div>
      </div>
      {error && <p className="error-banner" role="alert">{error}</p>}
      {attention.length > 0 && <section className="border-b border-line py-5" aria-label="Needs attention"><div className="mb-3 flex items-center gap-2"><CircleAlert className="text-[#cabd8a]" size={15} /><h2 className="text-sm font-medium">Needs attention</h2><span className="font-mono text-[10px] text-tertiary">{attention.filter((item) => !item.acknowledged).length}</span></div><div className="grid gap-2 xl:grid-cols-2">{attention.map((item) => { const approval = item.approvalRequestId ? pendingApprovals.get(item.approvalRequestId) : undefined; return <article className={`bg-surface p-3 ${item.acknowledged ? "opacity-55" : ""}`} key={item.key}><div className="flex items-start gap-3"><span className={`mt-1 size-2 shrink-0 rounded-full ${item.reason === "failed" ? "bg-failed" : item.reason === "completed_unreviewed" ? "bg-complete" : "bg-[#cabd8a]"}`} /><div className="min-w-0 flex-1"><strong className="block truncate text-sm">{item.title}</strong><p className="mt-1 text-xs text-secondary">{item.detail}</p></div><button aria-label={`Open ${item.title}`} className="icon-button" onClick={() => onOpen?.(item.taskId, item.runId)} type="button"><ExternalLink size={13} /></button></div><div className="mt-3 flex gap-2">{approval && <><button className="button-primary" onClick={() => decide(approval.id, "approved")} type="button"><Check size={13} />Approve</button><button className="button-danger" onClick={() => decide(approval.id, "denied")} type="button"><X size={13} />Deny</button></>} {!item.acknowledged && <button className="button-secondary ml-auto" onClick={() => acknowledge(item)} type="button">Acknowledge</button>}</div></article>; })}</div></section>}
      {!events.length ? <div className="relative min-h-64 py-16 pl-11">
        <span className="absolute bottom-0 left-[17px] top-0 w-px bg-line" aria-hidden="true" />
        <span className="absolute left-3 top-[70px] grid size-3 place-items-center rounded-full border border-line-strong bg-app" aria-hidden="true">
          <span className="size-1 rounded-full bg-tertiary" />
        </span>
        <div className="flex items-start gap-3">
          <Activity className="mt-0.5 text-secondary" aria-hidden="true" size={16} strokeWidth={1.5} />
          <div>
            <h3 className="text-sm font-medium text-primary">No activity yet</h3>
            <p className="mt-1 max-w-xl text-[13px] leading-5 text-tertiary">Run activity, file changes, context sharing, and lifecycle decisions will appear here in order.</p>
          </div>
        </div>
      </div> : <section aria-label="Project timeline" className="py-4"><ol>{events.map((event, index) => { const item = describe(event, taskNames.get(event.taskId ?? "")); return <li className="virtual-row grid grid-cols-[5.5rem_2rem_minmax(0,1fr)_2rem] gap-2" key={event.id}><time className="pt-4 text-right font-mono text-[10px] text-tertiary" dateTime={event.createdAt}>{time(event.createdAt)}</time><div className="relative flex justify-center"><span aria-hidden="true" className={`absolute left-1/2 w-px -translate-x-1/2 bg-line ${index === 0 ? "top-4" : "top-0"} ${index === events.length - 1 ? "bottom-auto h-4" : "bottom-0"}`} /><span className={`relative mt-3 grid size-7 place-items-center rounded-full border bg-app ${item.tone}`} aria-hidden="true">{item.icon}</span></div><div className="min-w-0 border-b border-line py-3"><strong className="block text-sm font-medium text-primary">{item.title}</strong><p className="mt-1 truncate text-xs leading-5 text-secondary">{item.detail}</p><span className="mt-1 block font-mono text-[10px] text-tertiary">{scope === "global" && `${event.projectName} · `}#{event.sequence}</span></div><div className="border-b border-line py-3">{event.taskId && <button aria-label={`Open ${taskNames.get(event.taskId) ?? "task"}`} className="icon-button" onClick={() => onOpen?.(event.taskId!, event.runId)} type="button"><ExternalLink aria-hidden="true" size={13} /></button>}</div></li>})}</ol></section>}
    </div>
  );
}

function describe(event: TimelineEvent, task?: string) {
  const payload = event.payload ?? {};
  const status = String(payload.to ?? "");
  const subject = task ?? "Task";
  if (event.eventType === "task.created") return { title: "Task created", detail: String(payload.title ?? subject), icon: <ListTodo size={13} />, tone: "border-line-strong text-secondary" };
  if (event.eventType === "task.status_changed") return { title: `${subject} moved to ${stage(status)}`, detail: payload.from ? `${stage(String(payload.from))} → ${stage(status)}` : `Task status changed`, icon: <ListTodo size={13} />, tone: tone(status) };
  if (event.eventType === "context.shared") return { title: "Context shared", detail: `${String(payload.kind ?? "Context")} sent to an agent`, icon: <MessageSquareText size={13} />, tone: "border-line-strong text-secondary" };
  if (event.eventType.startsWith("approval.")) return { title: event.eventType === "approval.requested" ? "Approval requested" : `Approval ${event.eventType.split(".")[1]}`, detail: `${subject} · ${String(payload.action ?? "Agent action")}`, icon: <GitPullRequest size={13} />, tone: "border-waiting text-waiting" };
  if (event.eventType.startsWith("agent.reported_")) return { title: event.eventType.replace("agent.reported_", "Agent ").replaceAll("_", " "), detail: String(payload.detail ?? subject), icon: <Bot size={13} />, tone: "border-line-strong text-secondary" };
  if (event.eventType.startsWith("run.")) return { title: status ? `Agent ${stage(status).toLowerCase()}` : `Agent ${event.eventType.split(".")[1].replaceAll("_", " ")}`, detail: String(payload.error ?? payload.instruction ?? subject), icon: <Bot size={13} />, tone: tone(status || event.eventType.split(".")[1]) };
  return { title: event.eventType.split(".").map(stage).join(" · "), detail: subject, icon: <Activity size={13} />, tone: "border-line-strong text-secondary" };
}
function stage(value: string) { return value.replaceAll("_", " ").replace(/^./, (letter) => letter.toUpperCase()); }
function tone(status: string) { if (["failed", "cancelled"].includes(status)) return "border-failed text-failed"; if (["succeeded", "approved", "merged", "archived", "review"].includes(status)) return "border-complete text-complete"; if (status === "waiting") return "border-waiting text-waiting"; return "border-line-strong text-secondary"; }
function time(value: string) { const date = new Date(value); return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(date); }
function errorMessage(error: unknown) { return error && typeof error === "object" && "message" in error ? String(error.message) : String(error); }
function notify(items: AttentionItem[], notified: Set<string>, onOpen?: (taskId: string, runId?: string | null) => void) {
  if (!("Notification" in window) || Notification.permission !== "granted") return;
  for (const item of items.filter((candidate) => !candidate.acknowledged && !notified.has(`${candidate.key}:${candidate.stateFingerprint}`))) { const fingerprint = `${item.key}:${item.stateFingerprint}`; notified.add(fingerprint); claimAttentionNotification(item.key, item.stateFingerprint).then((claimed) => { if (!claimed) return; const notification = new Notification(`SubShell · ${item.title}`, { body: item.detail, tag: item.key }); notification.onclick = () => { window.focus(); onOpen?.(item.taskId, item.runId); notification.close(); }; }).catch(() => notified.delete(fingerprint)); }
}
