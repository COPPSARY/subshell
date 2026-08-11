import { useEffect, useState } from "react";
import { Check, FileDiff, GitBranch, Plus, RotateCcw, Square, TerminalSquare } from "lucide-react";
import { ProviderIcon } from "../providers";
import { readRunDiff } from "./api";
import type { Run, RunDiff } from "./model";
import { agentLabel, statusDot } from "./TaskOverview";
import { RunTerminal, type SubscribeRunOutput } from "./RunTerminal";

type Props = {
  activeRunId: string;
  baseBranch: string;
  baseRevision: string;
  onSelectRun: (id: string) => void;
  onNewSession: () => void;
  onComplete: (id: string) => void;
  onResume: (id: string) => void;
  onStop: (id: string) => void;
  runs: Run[];
  subscribeOutput: SubscribeRunOutput;
};

export function RunInspector({ activeRunId, baseBranch, baseRevision, onComplete, onNewSession, onResume, onSelectRun, onStop, runs, subscribeOutput }: Props) {
  const run = runs.find((item) => item.id === activeRunId) ?? runs[0];
  const runIndex = runs.indexOf(run);
  const [view, setView] = useState<"terminal" | "changes">("terminal");
  const { diff, error } = useRunDiff(run, view === "changes" ? 1000 : 4000);
  const running = run.status === "running";

  return <section className="flex min-h-0 flex-1 overflow-hidden bg-surface" aria-label="Agent workspace">
    <div className="grid min-h-0 flex-1 2xl:grid-cols-[minmax(0,1fr)_18rem]">
      <div className="flex min-h-0 min-w-0 flex-col border-line 2xl:border-r">
        <div className="flex shrink-0 items-center gap-1 border-b border-line bg-chrome px-2 py-1" role="tablist" aria-label="Session view">
          <button aria-selected={view === "terminal"} className="flex min-h-8 items-center gap-2 rounded px-2 text-xs text-secondary hover:bg-panel aria-selected:bg-panel aria-selected:text-primary" onClick={() => setView("terminal")} role="tab" type="button"><TerminalSquare size={13} />Terminal</button>
          <button aria-label="Changes" aria-selected={view === "changes"} className="flex min-h-8 items-center gap-2 rounded px-2 text-xs text-secondary hover:bg-panel aria-selected:bg-panel aria-selected:text-primary" onClick={() => setView("changes")} role="tab" type="button"><FileDiff size={13} />Changes <span className="font-mono text-[10px]">{diff?.files.length ?? 0}</span></button>
          <span className="ml-auto hidden truncate text-[11px] text-tertiary xl:block">{agentLabel(run, runIndex)} · {running ? "click terminal to type" : `session ${run.status}`}</span>{running && run.role !== "planner" && <button className="button-primary ml-2" onClick={() => onComplete(run.id)} title="End this CLI session and keep its changes for combined review" type="button"><Check size={12} />Finish run</button>}{running && <button className="button-danger ml-1" onClick={() => onStop(run.id)} type="button"><Square size={11} />Stop</button>}
        </div>
        {view === "terminal" ? <div className="relative min-h-0 flex-1" role="tabpanel" aria-label="Terminal"><RunTerminal interactive={running} runId={run.id} subscribe={subscribeOutput} />{!running && <div className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-line bg-chrome/95 px-3 py-2 text-xs text-secondary"><span className={`size-2 rounded-full ${statusDot(run.status)}`} /><span>This session is {run.status}. Its terminal is read-only.</span><div className="ml-auto flex gap-2">{run.role !== "planner" && ["failed", "cancelled"].includes(run.status) && Boolean(diff?.files.length) && <button className="button-primary" onClick={() => onComplete(run.id)} type="button"><Check size={13} />Keep changes for review</button>}{run.canResume ? <button className="button-secondary" onClick={() => onResume(run.id)} type="button"><RotateCcw size={13} />Resume session</button> : <button className="button-secondary" onClick={onNewSession} type="button"><Plus size={13} />New session</button>}</div></div>}</div> : <div className="min-h-0 flex-1" role="tabpanel" aria-label="Changes"><Changes diff={diff} error={error} /></div>}
      </div>

      <aside className="hidden min-w-0 overflow-y-auto bg-chrome p-4 2xl:block" aria-label="Agent details">
        <div className="flex items-center justify-between"><h3 className="text-xs font-semibold uppercase tracking-wider text-secondary">Agents</h3><span className="font-mono text-[10px] text-tertiary">{runs.length}</span></div>
        <div className="mt-3 grid gap-1">{runs.map((item, index) => <button aria-current={item.id === run.id ? "true" : undefined} className="flex min-h-14 w-full items-center gap-3 rounded-md px-2 text-left hover:bg-panel aria-[current=true]:bg-panel" key={item.id} onClick={() => onSelectRun(item.id)} type="button"><ProviderIcon className="text-secondary" name={item.providerName} size={15} /><span className="min-w-0 flex-1"><strong className="block truncate text-xs font-medium">{index === 0 ? "Lead" : `Agent ${index + 1}`}</strong><span className="block truncate text-[11px] text-tertiary">{item.providerName}</span></span><span className="flex items-center gap-1.5 text-[10px] uppercase text-secondary"><span aria-hidden="true" className={`size-1.5 rounded-full ${statusDot(item.status)}`} />{item.status}</span></button>)}</div>

        <div className="mt-5 border-t border-line pt-4"><h3 className="text-xs font-semibold uppercase tracking-wider text-secondary">Workspace</h3><p className="mt-2 text-xs leading-5 text-tertiary">Changes are isolated from your opened repository.</p></div>
        <dl className="mt-4 grid gap-4 text-xs">
          <div><dt className="text-tertiary">Status</dt><dd className="mt-1 text-primary">{run.status}</dd></div>
          <div><dt className="text-tertiary">Base</dt><dd className="mt-1 flex items-center gap-1.5 font-mono text-[11px] text-secondary"><GitBranch size={12} />{baseBranch} · {baseRevision.slice(0, 8)}</dd></div>
          <div><dt className="text-tertiary">Worktree</dt><dd className="mt-1 break-all font-mono text-[10px] leading-4 text-secondary">{run.worktreePath ?? "Preparing…"}</dd></div>
          {(run.reportedInputTokens != null || run.reportedOutputTokens != null) && <div><dt className="text-tertiary">Provider-reported usage</dt><dd className="mt-1 font-mono text-[11px] text-secondary">{run.reportedInputTokens ?? "?"} in · {run.reportedOutputTokens ?? "?"} out</dd></div>}
        </dl>
        <div className="mt-5 border-t border-line pt-4"><button className="flex w-full items-center text-left text-xs font-medium text-primary" onClick={() => setView("changes")} type="button"><span className="flex-1">Changed files</span><span className="font-mono text-tertiary">{diff?.files.length ?? 0}</span></button>
          {error ? <p className="mt-2 text-xs text-failed">Could not read changes.</p> : diff?.files.length ? <ul className="mt-2 max-h-48 overflow-auto">{diff.files.map((file) => <li className="truncate py-1 font-mono text-[10px] text-secondary" key={file}>{file}</li>)}</ul> : <p className="mt-2 text-xs text-tertiary">No changes yet.</p>}
        </div>
      </aside>
    </div>
  </section>;
}

function Changes({ diff, error }: { diff: RunDiff | null; error: string }) {
  if (error) return <p className="error-banner" role="alert">{error}</p>;
  if (!diff) return <p className="empty-row">Reading worktree changes…</p>;
  if (!diff.files.length) return <p className="empty-row">No file changes yet.</p>;
  return <div className="grid h-full min-h-0 grid-cols-[13rem_minmax(0,1fr)] bg-app"><ul className="overflow-auto border-r border-line py-2" aria-label="Changed files">{diff.files.map((file) => <li className="truncate px-3 py-1.5 font-mono text-[11px] text-secondary" key={file}>{file}</li>)}</ul><pre className="overflow-auto whitespace-pre p-4 font-mono text-xs leading-5 text-secondary">{diff.patch || "File metadata changed without a text patch."}{diff.truncated && "\n\n…diff truncated at 1 MiB"}</pre></div>;
}

function useRunDiff(run: Run, interval: number) {
  const [diff, setDiff] = useState<RunDiff | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let mounted = true;
    setDiff(null); setError("");
    const load = () => readRunDiff(run.id).then((result) => { if (mounted) { setDiff(result); setError(""); } }).catch((reason) => mounted && setError(String(reason)));
    load();
    const timer = run.status === "running" ? window.setInterval(load, interval) : undefined;
    return () => { mounted = false; if (timer) window.clearInterval(timer); };
  }, [interval, run.id, run.status]);
  return { diff, error };
}
