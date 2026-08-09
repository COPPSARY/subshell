import { useEffect, useState } from "react";
import { Plus, Play, Square, X } from "lucide-react";
import { listContextSources, previewContext, type ContextPreview } from "../context";
import { listProviders, type GenericProfile } from "../providers";
import type { Project } from "../projects";
import type { Task } from "../tasks";
import { previewRunEnvironment, startRuns, stopRun } from "./api";
import type { Run, RunEvent } from "./model";
import { RunTerminal } from "./RunTerminal";

type Draft = { providerId: string; instruction: string; selectedFiles: string[]; pattern: string; environmentFiles: string; environmentPreview: string[] | null; preview: ContextPreview | null };
const draft = (): Draft => ({ providerId: "", instruction: "", selectedFiles: [], pattern: "", environmentFiles: "", environmentPreview: [], preview: null });

export function RunWorkspace({ project, task }: { project: Project; task: Task }) {
  const [providers, setProviders] = useState<GenericProfile[]>([]);
  const [sources, setSources] = useState<string[]>([]);
  const [drafts, setDrafts] = useState<Draft[]>([draft()]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [chunks, setChunks] = useState<Record<string, Uint8Array[]>>({});
  const [error, setError] = useState("");
  useEffect(() => { Promise.all([listProviders(), listContextSources(project.id)]).then(([nextProviders, nextSources]) => { setProviders(nextProviders); setSources(nextSources); setDrafts((items) => items.map((item) => ({ ...item, providerId: item.providerId || nextProviders[0]?.id || "" }))); }).catch((reason) => setError(String(reason))); }, [project.id]);

  function update(index: number, values: Partial<Draft>) { setDrafts((items) => items.map((item, itemIndex) => itemIndex === index ? { ...item, ...values } : item)); }
  async function preview(index: number) { const item = drafts[index]; try { update(index, { preview: await previewContext({ taskId: task.id, instruction: item.instruction, selectedFiles: item.selectedFiles, pattern: item.pattern || null }) }); } catch (reason) { setError(String(reason)); } }
  async function previewEnvironment(index: number) { try { const result = await previewRunEnvironment(project.id, lines(drafts[index].environmentFiles)); update(index, { environmentPreview: result.files }); } catch (reason) { setError(String(reason)); } }
  function event(event: RunEvent) { if (event.type === "output") setChunks((current) => ({ ...current, [event.runId]: [...(current[event.runId] ?? []), new Uint8Array(event.bytes)] })); else if (event.type === "statusChanged") setRuns((items) => items.map((run) => run.id === event.runId ? { ...run, status: event.status } : run)); else if (event.type === "failed") setError(event.error.message); }
  async function start() { const incomplete = drafts.some((item) => !item.providerId || !item.preview || item.environmentPreview === null); if (incomplete) { setError("Choose a provider and preview context and environment files for every assignment."); return; } setError(""); try { const started = await startRuns(task.id, drafts.map((item) => ({ providerId: item.providerId, instruction: item.instruction, contextToken: item.preview!.token, approvedContext: item.preview!.content, environmentFiles: lines(item.environmentFiles) })), event); setRuns(started); } catch (reason) { setError(String(reason)); } }
  async function stop(runId: string) { await stopRun(runId); setRuns((items) => items.map((run) => run.id === runId ? { ...run, status: "cancelled" } : run)); }

  return <section className="mt-6 border-t border-line pt-5" aria-label="Agent assignments">
    <div className="mb-4 flex items-center justify-between"><div><h2 className="text-base font-medium">Run agents</h2><p className="mt-1 text-sm text-secondary">Each assignment gets its own worktree, config folder, port, context snapshot, and log.</p></div><button className="button-primary" onClick={start} type="button"><Play size={14} />Run {drafts.length} agent{drafts.length > 1 ? "s" : ""}</button></div>
    {error && <p className="error-banner" role="alert">{error}</p>}
    <div className="grid gap-4 xl:grid-cols-2">{drafts.map((item, index) => <article className="form-panel" key={index}>
      <div className="flex items-center justify-between"><h3 className="font-medium">Assignment {index + 1}</h3>{drafts.length > 1 && <button className="icon-button" aria-label={`Remove assignment ${index + 1}`} onClick={() => setDrafts((items) => items.filter((_, itemIndex) => itemIndex !== index))} type="button"><X size={14} /></button>}</div>
      <label>CLI profile<select value={item.providerId} onChange={(event) => update(index, { providerId: event.target.value, preview: null })}><option value="">Choose a profile</option>{providers.map((profile) => <option key={profile.id} value={profile.id}>{profile.displayName}</option>)}</select></label>
      <label>Assignment instruction<textarea rows={3} value={item.instruction} onChange={(event) => update(index, { instruction: event.target.value, preview: null })} /></label>
      <label>Optional source pattern<input placeholder="src/*.rs" value={item.pattern} onChange={(event) => update(index, { pattern: event.target.value, preview: null })} /></label>
      <fieldset><legend>Context files</legend><div className="file-picker">{sources.map((source) => <label className="check-row" key={source}><input checked={item.selectedFiles.includes(source)} onChange={(event) => update(index, { selectedFiles: event.target.checked ? [...item.selectedFiles, source] : item.selectedFiles.filter((file) => file !== source), preview: null })} type="checkbox" />{source}</label>)}</div></fieldset>
      <label>Environment files (one relative path per line)<textarea rows={2} value={item.environmentFiles} onChange={(event) => update(index, { environmentFiles: event.target.value, environmentPreview: null })} /></label>
      <div className="flex items-center gap-2"><button className="button-secondary" onClick={() => previewEnvironment(index)} type="button">Preview files</button>{item.environmentPreview && <span className="text-xs text-secondary">{item.environmentPreview.length ? item.environmentPreview.join(", ") : "No files will be copied"}</span>}</div>
      <button className="button-secondary" onClick={() => preview(index)} type="button">Preview context</button>
      {item.preview && <div className="grid gap-2"><p className="text-xs text-secondary">{item.preview.manifest.totalBytes.toLocaleString()} / {item.preview.manifest.budgetBytes.toLocaleString()} bytes</p><ul className="font-mono text-[11px] text-secondary">{item.preview.manifest.entries.map((entry) => <li key={entry.source}>{entry.included ? "Included" : `Omitted (${entry.reason})`} · {entry.source} · {entry.bytes} B</li>)}</ul><textarea aria-label={`Editable context for assignment ${index + 1}`} className="font-mono text-xs" rows={12} value={item.preview.content} onChange={(event) => update(index, { preview: { ...item.preview!, content: event.target.value } })} /></div>}
    </article>)}</div>
    <button className="button-secondary mt-4" onClick={() => setDrafts((items) => [...items, { ...draft(), providerId: providers[0]?.id ?? "" }])} type="button"><Plus size={14} />Add assignment</button>
    {runs.length > 0 && <div className="mt-6 grid gap-4 xl:grid-cols-2">{runs.map((run) => <article className="overflow-hidden rounded-md border border-line" key={run.id}><header className="flex min-h-12 items-center gap-3 bg-panel px-3"><strong className="text-sm">{run.providerName}</strong><span className="status-pill">{run.status}</span><span className="ml-auto font-mono text-[11px] text-tertiary">:{run.port ?? "—"}</span>{run.status === "running" && <button className="button-danger" onClick={() => stop(run.id)} type="button"><Square size={12} />Stop</button>}</header><RunTerminal chunks={chunks[run.id] ?? []} runId={run.id} /><footer className="truncate border-t border-line px-3 py-2 font-mono text-[10px] text-tertiary" title={run.worktreePath ?? ""}>{run.worktreePath}</footer></article>)}</div>}
  </section>;
}
function lines(value: string) { return value.split("\n").map((line) => line.trim()).filter(Boolean); }
