import { useEffect, useRef, useState } from "react";
import { ExternalLink, Play, RotateCcw, Square, Trash2 } from "lucide-react";
import type { Review } from "../review";
import { closePreview, getPreview, preparePreview, readPreviewLog, restartPreview, startPreview, stopPreview } from "./api";
import type { Preview } from "./model";

export function PreviewPanel({ review, onCombinedPatch }: { review: Review; onCombinedPatch: (patch: string) => void }) {
  const [scope, setScope] = useState("combined");
  const [sessions, setSessions] = useState<Record<string, Preview>>({});
  const [logs, setLogs] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const cursors = useRef(new Map<string, number>());
  const sessionsRef = useRef(sessions);
  const current = sessions[scope];

  useEffect(() => { sessionsRef.current = sessions; }, [sessions]);
  useEffect(() => {
    setScope("combined"); setSessions({}); setLogs({}); cursors.current.clear();
    return () => { Object.values(sessionsRef.current).forEach((session) => { void closePreview(session.id); }); };
  }, [review.id]);

  useEffect(() => {
    if (!current) return;
    let active = true;
    const refresh = async () => {
      try {
        const [next, chunk] = await Promise.all([
          getPreview(current.id),
          readPreviewLog(current.id, cursors.current.get(current.id) ?? 0),
        ]);
        if (!active) return;
        setSessions((items) => ({ ...items, [scope]: next }));
        if (chunk.content) setLogs((items) => ({ ...items, [current.id]: (items[current.id] ?? "") + chunk.content }));
        cursors.current.set(current.id, chunk.cursor);
      } catch (reason) {
        if (active) setError(errorMessage(reason));
      }
    };
    void refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => { active = false; window.clearInterval(timer); };
  }, [current?.id, scope]);

  async function act(action: () => Promise<Preview>) {
    setBusy(true); setError("");
    try { const next = await action(); setSessions((items) => ({ ...items, [scope]: next })); }
    catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(false); }
  }

  async function prepare() {
    setBusy(true); setError("");
    try {
      const next = await preparePreview(review.id, review.fingerprint, scope === "combined" ? null : scope);
      setSessions((items) => ({ ...items, [scope]: next }));
      if (!next.runId) onCombinedPatch(next.combinedPatch);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(false); }
  }

  async function close() {
    if (!current) return;
    setBusy(true); setError("");
    try {
      await closePreview(current.id);
      setSessions((items) => { const next = { ...items }; delete next[scope]; return next; });
      setLogs((items) => { const next = { ...items }; delete next[current.id]; return next; });
      cursors.current.delete(current.id);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(false); }
  }

  return <section className="mt-5" aria-labelledby="preview-heading">
    <div className="flex flex-wrap items-end gap-3">
      <div className="min-w-48">
        <h3 className="text-sm font-medium" id="preview-heading">Application preview</h3>
        <label className="mt-2 block text-[11px] text-tertiary" htmlFor="preview-scope">Scope</label>
        <select className="mt-1 h-8 w-full rounded-md border border-line-strong bg-app px-2 text-xs" id="preview-scope" onChange={(event) => setScope(event.target.value)} value={scope}>
          <option value="combined">Combined application</option>
          {review.runs.map((run) => <option key={run.runId} value={run.runId}>{run.title}</option>)}
        </select>
      </div>
      {!current && <button className="button-primary" disabled={busy} onClick={prepare} type="button"><Play size={14} />Prepare preview</button>}
      {current?.status === "ready" && <button className="button-primary" disabled={busy} onClick={() => act(() => startPreview(current.id, current.command.fingerprint))} type="button"><Play size={14} />Run command</button>}
      {current && ["failed", "stopped"].includes(current.status) && <button className="button-primary" disabled={busy} onClick={() => act(() => restartPreview(current.id))} type="button"><RotateCcw size={14} />Restart</button>}
      {current && ["starting", "running"].includes(current.status) && <button className="button-secondary" disabled={busy} onClick={() => act(() => stopPreview(current.id))} type="button"><Square size={13} />Stop</button>}
      {current && <button className="button-secondary" disabled={busy} onClick={close} type="button"><Trash2 size={13} />Close</button>}
      {current && <span className="status-pill" role="status">{current.status} · {current.port}</span>}
    </div>
    {error && <p className="error-banner" role="alert">{error}</p>}
    {current && <div className="mt-3 border-y border-line bg-panel px-3 py-2">
      <p className="text-[10px] font-semibold uppercase tracking-wider text-tertiary">Confirm before execution</p>
      <code className="mt-1 block overflow-x-auto whitespace-pre py-1 font-mono text-[11px] text-secondary">{current.command.display}</code>
      <p className="mt-1 truncate font-mono text-[10px] text-tertiary" title={current.command.workingDirectory}>{current.command.workingDirectory}</p>
    </div>}
    {current?.error && <p className="error-banner" role="alert">{current.error}</p>}
    {current?.status === "running" && <div className="mt-3">
      <div className="mb-2 flex items-center justify-between"><span className="text-xs text-secondary">{current.scopeLabel}</span><a className="button-secondary" href={current.url} rel="noreferrer" target="_blank"><ExternalLink size={13} />Open in browser</a></div>
      <iframe className="h-[30rem] w-full border border-line bg-white" sandbox="allow-forms allow-modals allow-popups allow-same-origin allow-scripts" src={current.url} title={`${current.scopeLabel} preview`} />
    </div>}
    {current && <div className="mt-3"><h4 className="text-xs font-medium">Server logs</h4><pre aria-live="polite" className="mt-2 max-h-56 min-h-20 overflow-auto rounded-md bg-app p-3 font-mono text-[11px] leading-5 text-secondary">{logs[current.id] || "Waiting for output…"}</pre></div>}
  </section>;
}

function errorMessage(error: unknown) {
  if (!error || typeof error !== "object") return String(error);
  const message = "message" in error ? String(error.message) : String(error);
  const files = "details" in error && error.details && typeof error.details === "object" && "files" in error.details && Array.isArray(error.details.files) ? error.details.files.join(", ") : "";
  return files ? `${message}: ${files}` : message;
}
