import { useEffect, useState } from "react";
import { AlertTriangle, Check, GitMerge, ListPlus, RotateCcw } from "lucide-react";
import { errorMessage } from "../../shared/error";
import { PreviewPanel } from "../preview";
import { enqueueMerge } from "../workspace";
import { approveReview, getReview, mergeReview, sendBackReview } from "./api";
import type { Review } from "./model";

export function ReviewView({ taskId, readOnly = false }: { taskId: string; readOnly?: boolean }) {
  const [review, setReview] = useState<Review | null>(null);
  const [feedback, setFeedback] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [mergedRevision, setMergedRevision] = useState("");
  const [queued, setQueued] = useState(false);
  const [materializedPatch, setMaterializedPatch] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setMaterializedPatch(null);
    getReview(taskId).then((value) => active && setReview(value)).catch((reason) => active && setError(errorMessage(reason)));
    return () => { active = false; };
  }, [taskId]);

  async function act(action: () => Promise<Review>) {
    setBusy(true); setError("");
    try { setReview(await action()); } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(false); }
  }

  async function merge() {
    if (!review) return;
    setBusy(true); setError("");
    try { setMergedRevision(await mergeReview(review.id, review.fingerprint)); } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(false); }
  }
  async function queueMerge() {
    if (!review) return;
    setBusy(true); setError("");
    try { await enqueueMerge(review.id, review.fingerprint); setQueued(true); } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(false); }
  }

  if (error && !review) return <p className="error-banner" role="alert">{error}</p>;
  if (!review) return <p className="empty-row">Assembling the exact combined review…</p>;
  return <div className="min-h-0 flex-1 overflow-auto bg-surface p-5">
    <header className="flex flex-wrap items-start gap-4 border-b border-line pb-4">
      <div className="min-w-0 flex-1"><p className="text-[11px] font-semibold uppercase tracking-wider text-tertiary">Combined review · attempt {review.attemptNumber}</p><h2 className="mt-1 text-xl font-medium">{review.runs.length} agent result{review.runs.length === 1 ? "" : "s"}</h2><p className="mt-1 font-mono text-[11px] text-tertiary">Base {review.baseRevision.slice(0, 8)} · {review.fingerprint.slice(0, 12)}</p></div>
      {review.decision === "pending" && <button className="button-primary" disabled={busy} onClick={() => act(() => approveReview(review.id, review.fingerprint))} type="button"><Check size={14} />Approve exact result</button>}
      {review.decision === "approved" && readOnly && <span className="status-pill text-complete">Merged and archived</span>}
      {review.decision === "approved" && !readOnly && !mergedRevision && !queued && <><button className="button-secondary" disabled={busy} onClick={queueMerge} type="button"><ListPlus size={14} />Add to merge queue</button><button className="button-primary" disabled={busy} onClick={merge} type="button"><GitMerge size={14} />Merge locally</button></>}
      {queued && <span className="status-pill text-waiting">Queued for merge</span>}
      {mergedRevision && <span className="status-pill text-complete">Merged {mergedRevision.slice(0, 8)}</span>}
    </header>
    {error && <p className="error-banner mt-4" role="alert">{error}</p>}
    <PreviewPanel onCombinedPatch={setMaterializedPatch} review={review} />
    {review.conflicts.length > 0 && <section className="mt-5"><h3 className="flex items-center gap-2 text-sm font-medium"><AlertTriangle className="text-waiting" size={15} />Check these overlaps</h3><ul className="mt-2 divide-y divide-line border-y border-line">{review.conflicts.map((flag, index) => <li className="py-2 text-xs" key={`${flag.category}-${index}`}><strong className="mr-2 uppercase text-waiting">{flag.category.replaceAll("_", " ")}</strong><span className="text-secondary">{flag.evidence}</span></li>)}</ul></section>}
    <section className="mt-5 grid gap-3 xl:grid-cols-2">{review.runs.map((run) => <article className="rounded-md bg-panel p-3" key={run.runId}><div className="flex items-center justify-between gap-2"><h3 className="text-sm font-medium">{run.title}</h3><span className="text-[10px] text-tertiary">{run.providerName}</span></div><p className="mt-1 text-xs text-secondary">{run.explanation || run.instruction}</p><ul className="mt-3 font-mono text-[11px] text-tertiary">{run.files.map((file) => <li key={file}>{file}</li>)}</ul><p className="mt-3 text-[10px] text-tertiary">Context {run.contextManifest.totalBytes?.toLocaleString() ?? "unknown"} bytes · {(run.contextManifest.entries ?? []).filter((entry) => entry.included).length} sources</p></article>)}</section>
    {review.validationEvidence.length > 0 && <section className="mt-5"><h3 className="text-sm font-medium">Validation evidence</h3><ul className="mt-2 divide-y divide-line border-y border-line">{review.validationEvidence.map((item, index) => <li className="py-2 text-xs text-secondary" key={`${item.runId}-${index}`}>{item.summary}</li>)}</ul></section>}
    <section className="mt-5"><h3 className="text-sm font-medium">{materializedPatch === null ? "Agent patches" : "Exact combined diff"}</h3><pre className="mt-2 max-h-[32rem] overflow-auto rounded-md bg-app p-4 font-mono text-[11px] leading-5 text-secondary">{(materializedPatch ?? review.combinedPatch) || "No file changes."}</pre></section>
    {review.decision === "pending" && <section className="mt-5 border-t border-line pt-5"><div className="max-w-3xl"><h3 className="text-sm font-medium text-primary">Request changes</h3><p className="mt-1 text-xs leading-5 text-tertiary">Explain what the agents should revise before you approve this result.</p><label className="mt-4 block text-xs font-medium text-secondary" htmlFor="review-feedback">Feedback</label><textarea className="mt-2 min-h-24 w-full resize-y rounded-md border border-line-strong bg-app px-3 py-2 text-sm leading-5 text-primary focus-visible:outline-line-strong" id="review-feedback" onChange={(event) => setFeedback(event.target.value)} placeholder="Describe the required changes…" rows={4} value={feedback} /><div className="mt-3 flex justify-end"><button className="button-secondary" disabled={busy || !feedback.trim()} onClick={() => act(() => sendBackReview(review.id, review.fingerprint, feedback))} type="button"><RotateCcw size={14} />Send back</button></div></div></section>}
  </div>;
}
