import { useEffect, useRef } from "react";

type Decision = "preserve" | "stop" | "cancel";

export function SafeQuitDialog({ activeRuns, onDecision }: { activeRuns: number; onDecision: (decision: Decision) => void }) {
  const dialog = useRef<HTMLElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    requestAnimationFrame(() => cancel.current?.focus());
    return () => previous?.focus();
  }, []);
  function keydown(event: React.KeyboardEvent) {
    if (event.key === "Escape") { event.preventDefault(); onDecision("cancel"); return; }
    if (event.key !== "Tab") return;
    const buttons = [...(dialog.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const first = buttons[0]; const last = buttons.at(-1);
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  }
  return <div className="fixed inset-0 z-50 grid place-items-center overscroll-contain bg-black/70 p-4"><section aria-labelledby="quit-title" aria-modal="true" className="form-panel w-full max-w-md" onKeyDown={keydown} ref={dialog} role="dialog"><div><h2 className="text-base font-medium text-primary" id="quit-title">Agents are still running</h2><p className="mt-2 text-sm leading-6 text-secondary">{activeRuns} active run{activeRuns === 1 ? "" : "s"} would be interrupted. Keep SubShell minimized, stop the runs safely, or cancel.</p></div><div className="flex flex-wrap justify-end gap-2"><button className="button-secondary" onClick={() => onDecision("cancel")} ref={cancel} type="button">Cancel</button><button className="button-secondary" onClick={() => onDecision("preserve")} type="button">Keep running</button><button className="button-danger" onClick={() => onDecision("stop")} type="button">Stop and quit</button></div></section></div>;
}
