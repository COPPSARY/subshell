import { useCallback, useEffect, useRef, useState } from "react";
import { Check, CircleStop, CircleX, Clock3, FileDiff, ListTodo, Plus, Play, X } from "lucide-react";
import { listContextSources, previewContext, type ContextPreview } from "../context";
import { createProvider, detectProviders, listProviders, type GenericProfile } from "../providers";
import { ProviderIcon } from "../providers/ProviderIcon";
import type { Project } from "../projects";
import type { Task } from "../tasks";
import { ReviewView } from "../review";
import { applyEnvironmentProfile, listAgentTemplates, listEnvironmentProfiles, type AgentTemplate, type EnvironmentProfile } from "../workspace";
import { approveTaskPlan, completeRun, enqueueRuns, getTaskPlan, listRuns, previewRunEnvironment, rejectTaskPlan, resumeRun, retryRun, startRuns, stopRun } from "./api";
import type { Run, RunEvent, RunOutputChunk, TaskPlan } from "./model";
import { RunInspector } from "./RunInspector";
import { agentLabel, statusDot, TaskOverview } from "./TaskOverview";

type Draft = { providerId: string; instruction: string; role: string; selectedFiles: string[]; pattern: string; environmentFiles: string; environmentPreview: string[] | null; preview: ContextPreview | null };
const draft = (): Draft => ({ providerId: "", instruction: "", role: "implementer", selectedFiles: [], pattern: "", environmentFiles: "", environmentPreview: [], preview: null });

type Props = { project: Project; task: Task; autoStart?: boolean; initialRunId?: string | null; onAutoStartConsumed?: () => void; onActiveRunChange?: (runId: string | null) => void };

export function RunWorkspace({ project, task, autoStart = false, initialRunId, onAutoStartConsumed, onActiveRunChange }: Props) {
  const [providers, setProviders] = useState<GenericProfile[]>([]);
  const [templates, setTemplates] = useState<AgentTemplate[]>([]);
  const [environmentProfiles, setEnvironmentProfiles] = useState<EnvironmentProfile[]>([]);
  const [sources, setSources] = useState<string[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([draft()]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [plan, setPlan] = useState<TaskPlan | null>(null);
  const [planBusy, setPlanBusy] = useState(false);
  const outputListeners = useRef(new Map<string, (chunk: RunOutputChunk) => void>());
  const [activeRunId, setActiveRunId] = useState("");
  const [error, setError] = useState("");
  const [providersLoaded, setProvidersLoaded] = useState(false);
  const [autoLaunching, setAutoLaunching] = useState(autoStart);
  const [builderOpen, setBuilderOpen] = useState(false);
  const autoStarted = useRef(false);
  useEffect(() => { autoStarted.current = false; }, [task.id]);
  useEffect(() => { outputListeners.current.clear(); Promise.all([listProviders(), listContextSources(project.id), listRuns(task.id), getTaskPlan(task.id), listAgentTemplates(project.id), listEnvironmentProfiles(project.id)]).then(([nextProviders, nextSources, existingRuns, taskPlan, nextTemplates, nextEnvironmentProfiles]) => { setProviders(nextProviders); setTemplates(nextTemplates); setEnvironmentProfiles(nextEnvironmentProfiles); setProvidersLoaded(true); setSources(nextSources); setRuns(existingRuns); setPlan(taskPlan); setActiveRunId(initialRunId && existingRuns.some((run) => run.id === initialRunId) ? initialRunId : existingRuns.find((run) => ["queued", "preparing", "running", "waiting"].includes(run.status))?.id ?? existingRuns.find((run) => run.role !== "planner" && run.status !== "succeeded")?.id ?? (reviewIsReady(existingRuns) ? "" : existingRuns[0]?.id ?? "")); setDrafts((items) => items.map((item) => ({ ...item, providerId: item.providerId || nextProviders[0]?.id || "" }))); }).catch((reason) => setError(errorMessage(reason))); }, [project.id, task.id]);
  useEffect(() => { setActiveRunId(initialRunId && runs.some((run) => run.id === initialRunId) ? initialRunId : ""); }, [initialRunId]);
  useEffect(() => {
    let mounted = true;
    const refresh = () => Promise.all([listRuns(task.id), getTaskPlan(task.id)]).then(([items, taskPlan]) => { if (mounted) { setRuns(items); setPlan(taskPlan); } }).catch(() => undefined);
    const timer = window.setInterval(refresh, 2000);
    return () => { mounted = false; window.clearInterval(timer); };
  }, [task.id]);

  useEffect(() => {
    if (!autoStart || autoStarted.current || !providersLoaded) return;
    autoStarted.current = true;
    setAutoLaunching(true);
    const instruction = "Inspect this goal and repository, then submit a bounded parallel plan through SubShell. Do not edit files in the planner Run.";
    Promise.resolve(providers[0] ?? detectProviders().then((installed) => {
      const detected = installed[0];
      if (!detected) throw new Error("No supported AI CLI was found. Install Claude Code, Codex, Kiro, or Gemini, or add a Custom CLI in Providers.");
      return createProvider({ id: "", displayName: detected.displayName, providerType: detected.key, executablePath: detected.executablePath, arguments: detected.arguments, resumeArguments: detected.resumeArguments, promptMode: detected.promptMode, configRootEnvVar: detected.configRootEnvVar, configSourcePath: null, inheritUserHome: false });
    }))
      .then(async (provider) => {
        setProviders((current) => current.length ? current : [provider]);
        const context = await previewContext({ taskId: task.id, instruction, selectedFiles: [], pattern: null });
        setDrafts([{ ...draft(), providerId: provider.id, instruction, preview: context }]);
        return startRuns(task.id, [{ providerId: provider.id, instruction, role: "planner", title: "Plan the goal", contextToken: context.token, approvedContext: context.content, environmentFiles: [] }], event);
      })
      .then((started) => { const runId = started[0]?.id ?? ""; setRuns(started); setActiveRunId(runId); if (runId) onActiveRunChange?.(runId); })
      .catch((reason) => setError(String(reason)))
      .finally(() => { setAutoLaunching(false); onAutoStartConsumed?.(); });
  }, [autoStart, providersLoaded, providers, task.id]);

  function update(index: number, values: Partial<Draft>) { setDrafts((items) => items.map((item, itemIndex) => itemIndex === index ? { ...item, ...values } : item)); }
  async function preview(index: number) { const item = drafts[index]; try { update(index, { preview: await previewContext({ taskId: task.id, instruction: item.instruction, selectedFiles: item.selectedFiles, pattern: item.pattern || null }) }); } catch (reason) { setError(String(reason)); } }
  async function previewEnvironment(index: number) { try { const result = await previewRunEnvironment(project.id, lines(drafts[index].environmentFiles)); update(index, { environmentPreview: result.files }); } catch (reason) { setError(String(reason)); } }
  function event(event: RunEvent) {
    if (event.type === "output") outputListeners.current.get(event.runId)?.({ bytes: event.bytes, cursor: event.cursor });
    else if (event.type === "statusChanged") {
      setRuns((items) => items.map((run) => run.id === event.runId ? { ...run, status: event.status } : run));
      if (["cancelled", "failed", "succeeded"].includes(event.status)) setActiveRunId((current) => {
        if (current !== event.runId) return current;
        onActiveRunChange?.(null);
        return "";
      });
    } else if (event.type === "failed") setError(event.error.message);
  }
  async function start(queued = false) { const incomplete = drafts.some((item) => !item.providerId || !item.preview || item.environmentPreview === null); if (incomplete) { setError("Choose a provider and preview context and environment files for every assignment."); return; } setError(""); try { const assignments = drafts.map((item) => ({ providerId: item.providerId, instruction: item.instruction, role: item.role, contextToken: item.preview!.token, approvedContext: item.preview!.content, environmentFiles: lines(item.environmentFiles) })); const started = queued ? await enqueueRuns(task.id, assignments) : await startRuns(task.id, assignments, event); const runId = queued ? "" : started[0]?.id ?? ""; setRuns(started); setActiveRunId(runId); onActiveRunChange?.(runId || null); setBuilderOpen(false); } catch (reason) { setError(String(reason)); } }
  async function stop(runId: string) { await stopRun(runId); setRuns((items) => items.map((run) => run.id === runId ? { ...run, status: "cancelled" } : run)); }
  async function complete(runId: string) { setError(""); try { await completeRun(runId); const next = await listRuns(task.id); setRuns(next); const unfinished = next.find((run) => run.role !== "planner" && run.status !== "succeeded"); selectRun(unfinished?.id ?? null); } catch (reason) { setError(errorMessage(reason)); } }
  async function resume(runId: string) { setError(""); try { const resumed = await resumeRun(runId, event); setRuns((items) => items.map((run) => run.id === runId ? resumed : run)); selectRun(runId); } catch (reason) { setError(String(reason)); } }
  async function retry(runId: string) { setError(""); try { const retried = await retryRun(runId, event); setRuns((items) => [...items, retried]); selectRun(retried.id); } catch (reason) { setError(errorMessage(reason)); } }
  async function approvePlan(fullAccess: boolean) { if (!plan) return; setPlanBusy(true); setError(""); try { setPlan(await approveTaskPlan(plan.id, fullAccess, event)); const next = await listRuns(task.id); setRuns(next); const firstExecutor = next.find((run) => run.role !== "planner" && ["queued", "preparing", "running", "waiting"].includes(run.status)); selectRun(firstExecutor?.id ?? null); } catch (reason) { setError(errorMessage(reason)); } finally { setPlanBusy(false); } }
  async function rejectPlan() { if (!plan) return; setPlanBusy(true); setError(""); try { setPlan(await rejectTaskPlan(plan.id)); setRuns(await listRuns(task.id)); selectRun(null); } catch (reason) { setError(errorMessage(reason)); } finally { setPlanBusy(false); } }
  function selectRun(runId: string | null) { setActiveRunId(runId ?? ""); onActiveRunChange?.(runId); }
  const subscribeOutput = useCallback((runId: string, listener: (chunk: RunOutputChunk) => void) => { outputListeners.current.set(runId, listener); return () => { if (outputListeners.current.get(runId) === listener) outputListeners.current.delete(runId); }; }, []);

  const showBuilder = (!runs.length && !autoLaunching) || builderOpen;
  const reviewReady = reviewIsReady(runs);
  return <section className="flex min-h-0 flex-1 flex-col pt-1" aria-label="Agent assignments">
    {runs.length ? <div className="flex h-9 shrink-0 items-center gap-1 border-b border-line px-1"><div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto" role="tablist" aria-label="Task and agent sessions"><button aria-selected={!activeRunId && !builderOpen} className="flex h-7 shrink-0 items-center gap-1.5 rounded px-2.5 text-[11px] text-tertiary outline-none hover:bg-panel hover:text-secondary focus-visible:ring-1 focus-visible:ring-line-strong aria-selected:bg-selected aria-selected:text-primary" onClick={() => { setBuilderOpen(false); selectRun(null); }} role="tab" type="button">{reviewReady ? <FileDiff aria-hidden="true" size={12} /> : <ListTodo aria-hidden="true" size={12} />}{reviewReady ? "Review" : "Overview"}</button>{runs.map((run, index) => <button aria-selected={run.id === activeRunId && !builderOpen} className="flex h-7 shrink-0 items-center gap-1.5 rounded px-2.5 text-[11px] text-tertiary outline-none hover:bg-panel hover:text-secondary focus-visible:ring-1 focus-visible:ring-line-strong aria-selected:bg-selected aria-selected:text-primary" key={run.id} onClick={() => { setBuilderOpen(false); selectRun(run.id); }} role="tab" title={`${agentLabel(run, index)} · ${run.instruction}`} type="button"><ProviderIcon aria-hidden="true" className="text-secondary" name={run.providerName} size={12} /><span>{sessionLabel(run, index)}</span><SessionStatus status={run.status} /></button>)}</div><button aria-label="New session" className="flex size-7 shrink-0 items-center justify-center rounded text-tertiary outline-none hover:bg-panel hover:text-primary focus-visible:ring-1 focus-visible:ring-line-strong" onClick={() => setBuilderOpen(true)} title="New session" type="button"><Plus aria-hidden="true" size={14} /></button></div> : <div className="mb-4 flex shrink-0 items-center justify-between"><div><h2 className="text-base font-medium">Run agents</h2><p className="mt-1 text-sm text-secondary">Each agent gets an isolated worktree and focused context.</p></div>{showBuilder && <div className="flex gap-2"><button className="button-secondary" onClick={() => start(true)} type="button"><Clock3 size={14} />Queue</button><button className="button-primary" onClick={() => start()} type="button"><Play size={14} />Run {drafts.length} agent{drafts.length > 1 ? "s" : ""}</button></div>}</div>}
    {plan?.status === "proposed" && activeRunId && <div className="flex min-h-10 shrink-0 items-center gap-2 border-b border-waiting/40 bg-[#17150f] px-3 text-xs"><span aria-hidden="true" className="size-1.5 rounded-full bg-waiting" /><strong className="font-medium text-primary">Plan ready</strong><span className="min-w-0 flex-1 truncate text-secondary">{plan.assignments.length} assignments need your approval</span><button className="button-secondary" onClick={() => { setBuilderOpen(false); selectRun(null); }} type="button">Review plan</button></div>}
    {error && <p className="error-banner" role="alert">{error}</p>}
    {autoLaunching && <p className="empty-row">Building focused context and starting the agent…</p>}
    {runs.length > 0 && !builderOpen && (activeRunId ? <RunInspector activeRunId={activeRunId} baseBranch={task.baseBranch} baseRevision={task.baseRevision} onComplete={complete} onNewSession={() => setBuilderOpen(true)} onResume={resume} onRetry={retry} onSelectRun={selectRun} onStop={stop} runs={runs} subscribeOutput={subscribeOutput} /> : plan?.status === "proposed" ? <TaskOverview onApprovePlan={approvePlan} onRejectPlan={rejectPlan} onSelectRun={selectRun} plan={plan} planBusy={planBusy} runs={runs} task={task} /> : reviewReady ? <ReviewView taskId={task.id} /> : <TaskOverview onSelectRun={selectRun} runs={runs} task={task} />)}
    {showBuilder && <div className="min-h-0 flex-1 overflow-auto pb-5"><div className="grid gap-4 xl:grid-cols-2">{drafts.map((item, index) => <article className="form-panel" key={index}>
      <div className="flex items-center justify-between"><h3 className="font-medium">Assignment {index + 1}</h3>{drafts.length > 1 && <button className="icon-button" aria-label={`Remove assignment ${index + 1}`} onClick={() => setDrafts((items) => items.filter((_, itemIndex) => itemIndex !== index))} type="button"><X size={14} /></button>}</div>
      {templates.length > 0 && <label>Agent template<select defaultValue="" onChange={(event) => { const template = templates.find((value) => value.id === event.target.value); if (template) update(index, { providerId: template.providerId ?? item.providerId, instruction: template.instruction, role: template.role, environmentFiles: template.environmentFiles.join("\n"), environmentPreview: null, preview: null }); }}><option value="">Custom assignment</option>{templates.map((template) => <option key={template.id} value={template.id}>{template.name}</option>)}</select></label>}
      <label>CLI profile<select value={item.providerId} onChange={(event) => update(index, { providerId: event.target.value, preview: null })}><option value="">Choose a profile</option>{providers.map((profile) => <option key={profile.id} value={profile.id}>{profile.displayName}</option>)}</select></label>
      <label>Agent role<select onChange={(event) => update(index, { role: event.target.value })} value={item.role}>{["implementer", "reviewer", "tester", "debugger", "research"].map((role) => <option key={role}>{role}</option>)}</select></label>
      <label>Assignment instruction<textarea rows={3} value={item.instruction} onChange={(event) => update(index, { instruction: event.target.value, preview: null })} /></label>
      <label>Optional source pattern<input placeholder="src/*.rs" value={item.pattern} onChange={(event) => update(index, { pattern: event.target.value, preview: null })} /></label>
      <fieldset><legend>Context files</legend><div className="file-picker">{sources.map((source) => <label className="check-row" key={source}><input checked={item.selectedFiles.includes(source)} onChange={(event) => update(index, { selectedFiles: event.target.checked ? [...item.selectedFiles, source] : item.selectedFiles.filter((file) => file !== source), preview: null })} type="checkbox" />{source}</label>)}</div></fieldset>
      <label>Environment files (one relative path per line)<textarea rows={2} value={item.environmentFiles} onChange={(event) => update(index, { environmentFiles: event.target.value, environmentPreview: null })} /></label>
      {environmentProfiles.length > 0 && <label>Environment profile<select defaultValue="" onChange={(event) => { const profile = environmentProfiles.find((value) => value.id === event.target.value); if (profile) void applyEnvironmentProfile(profile.id, task.id).then(() => update(index, { environmentFiles: profile.environmentFiles.join("\n"), environmentPreview: null })).catch((reason) => setError(errorMessage(reason))); }}><option value="">No saved profile</option>{environmentProfiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name} · {profile.validationCommands.length} checks</option>)}</select></label>}
      <div className="flex items-center gap-2"><button className="button-secondary" onClick={() => previewEnvironment(index)} type="button">Preview files</button>{item.environmentPreview && <span className="text-xs text-secondary">{item.environmentPreview.length ? item.environmentPreview.join(", ") : "No files will be copied"}</span>}</div>
      <button className="button-secondary" onClick={() => preview(index)} type="button">Preview context</button>
      {item.preview && <div className="grid gap-2"><p className="text-xs text-secondary">{item.preview.manifest.totalBytes.toLocaleString()} / {item.preview.manifest.budgetBytes.toLocaleString()} bytes</p><ul className="font-mono text-[11px] text-secondary">{item.preview.manifest.entries.map((entry) => <li key={entry.source}>{entry.included ? "Included" : `Omitted (${entry.reason})`} · {entry.source} · {entry.bytes} B</li>)}</ul><textarea aria-label={`Editable context for assignment ${index + 1}`} className="font-mono text-xs" rows={12} value={item.preview.content} onChange={(event) => update(index, { preview: { ...item.preview!, content: event.target.value } })} /></div>}
    </article>)}</div>
    <div className="mt-4 flex items-center gap-2"><button className="button-secondary" onClick={() => setDrafts((items) => [...items, { ...draft(), providerId: providers[0]?.id ?? "" }])} type="button"><Plus size={14} />Add assignment</button>{runs.length > 0 && <><button className="button-secondary ml-auto" onClick={() => start(true)} type="button"><Clock3 size={14} />Queue</button><button className="button-primary" onClick={() => start()} type="button"><Play size={14} />Run now</button></>}</div></div>}
  </section>;
}
function lines(value: string) { return value.split("\n").map((line) => line.trim()).filter(Boolean); }
function reviewIsReady(runs: Run[]) { const implementation = runs.filter((run) => run.role !== "planner"); return implementation.length > 0 && implementation.every((run) => run.status === "succeeded"); }
function sessionLabel(run: Run, index: number) { return run.role === "planner" ? "Planner" : run.title || (index === 0 ? "Lead" : `Agent ${index + 1}`); }
function errorMessage(error: unknown) { return error && typeof error === "object" && "message" in error ? String(error.message) : String(error); }
function SessionStatus({ status }: { status: string }) {
  if (status === "succeeded") return <span title="succeeded"><Check aria-hidden="true" className="text-complete" size={11} /><span className="sr-only">succeeded</span></span>;
  if (status === "cancelled") return <span title="stopped"><CircleStop aria-hidden="true" className="text-tertiary" size={11} /><span className="sr-only">stopped</span></span>;
  if (status === "failed") return <span title="failed"><CircleX aria-hidden="true" className="text-failed" size={11} /><span className="sr-only">failed</span></span>;
  return <span title={status}><span aria-hidden="true" className={`block size-1.5 rounded-full ${statusDot(status)}`} /><span className="sr-only">{status}</span></span>;
}
