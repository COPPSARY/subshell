import { useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { readRunOutput, readRunOutputTail, resizeRun, writeRunInput } from "./api";
import type { RunOutputChunk } from "./model";

export type SubscribeRunOutput = (runId: string, listener: (chunk: RunOutputChunk) => void) => () => void;

export function RunTerminal({ runId, subscribe, interactive = true }: { runId: string; subscribe: SubscribeRunOutput; interactive?: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | null>(null);

  useEffect(() => {
    let disposed = false;
    let cleanup = () => undefined;
    void Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit")]).then(([xterm, addon]) => {
      if (disposed || !host.current) return;
      const instance = new xterm.Terminal({ convertEol: true, cursorBlink: true, fontFamily: "ui-monospace, SFMono-Regular, monospace", fontSize: 13, screenReaderMode: true, theme: { background: "#08090b", foreground: "#d9dce2", cursor: "#849aff" } });
      const fit = new addon.FitAddon(); instance.loadAddon(fit); instance.open(host.current); fit.fit(); terminal.current = instance;
      let cursor = 0; let reading = false; let readAgain = false; let pending: RunOutputChunk[] = []; let outputFrame: number | undefined;
      const flush = () => {
        const waiting: RunOutputChunk[] = [];
        const writes: Uint8Array[] = [];
        let writeBytes = 0;
        for (const chunk of pending) {
          const start = chunk.cursor - chunk.bytes.length;
          if (chunk.cursor <= cursor) continue;
          if (start > cursor) { waiting.push(chunk); continue; }
          const offset = Math.max(0, cursor - start);
          const bytes = new Uint8Array(chunk.bytes.slice(offset));
          writes.push(bytes); writeBytes += bytes.length;
          cursor = chunk.cursor;
        }
        if (writeBytes) {
          const bytes = new Uint8Array(writeBytes);
          let offset = 0;
          for (const write of writes) { bytes.set(write, offset); offset += write.length; }
          instance.write(bytes);
        }
        pending = waiting;
        return waiting.length > 0;
      };
      const scheduleOutput = () => { outputFrame ??= window.requestAnimationFrame(() => { outputFrame = undefined; if (!reading && flush()) catchUp(); }); };
      const catchUp = () => {
        if (reading) { readAgain = true; return; }
        reading = true;
        readRunOutput(runId, cursor).then((output) => {
          if (disposed) return;
          if (output.bytes.length) instance.write(new Uint8Array(output.bytes));
          cursor = output.nextCursor;
          const hasGap = flush();
          if (output.bytes.length === 65536 || (hasGap && output.bytes.length > 0)) readAgain = true;
        }).catch(() => undefined).finally(() => { reading = false; if (readAgain) { readAgain = false; catchUp(); } else if (pending.length) scheduleOutput(); });
      };
      let recovery: number | undefined;
      let unsubscribe: () => void = () => undefined;
      readRunOutputTail(runId).catch(() => ({ bytes: [], nextCursor: 0 })).then((output) => {
        if (disposed) return;
        cursor = output.nextCursor;
        const ready = () => {
          if (disposed) return;
          instance.scrollToBottom();
          unsubscribe = subscribe(runId, (chunk) => { pending.push(chunk); scheduleOutput(); });
          catchUp();
        };
        if (output.bytes.length) instance.write(new Uint8Array(output.bytes), ready); else ready();
      }).finally(() => { if (!disposed && interactive) recovery = window.setInterval(catchUp, 2000); });
      let inputBuffer = ""; let inputFrame: number | undefined; let inputQueue = Promise.resolve();
      const flushInput = () => { inputFrame = undefined; const data = inputBuffer; inputBuffer = ""; if (data) inputQueue = inputQueue.then(() => writeRunInput(runId, Array.from(new TextEncoder().encode(data)))).catch(() => undefined); };
      const input = interactive ? instance.onData((data) => { inputBuffer += data; inputFrame ??= window.requestAnimationFrame(flushInput); }) : null;
      let resizeFrame: number | undefined; let lastRows = 0; let lastCols = 0;
      const resize = () => { resizeFrame ??= window.requestAnimationFrame(() => { resizeFrame = undefined; fit.fit(); if (interactive && (instance.rows !== lastRows || instance.cols !== lastCols)) { lastRows = instance.rows; lastCols = instance.cols; resizeRun(runId, instance.rows, instance.cols).catch(() => undefined); } }); };
      const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize); observer?.observe(host.current);
      cleanup = () => { if (recovery) window.clearInterval(recovery); if (outputFrame !== undefined) window.cancelAnimationFrame(outputFrame); if (inputFrame !== undefined) window.cancelAnimationFrame(inputFrame); if (resizeFrame !== undefined) window.cancelAnimationFrame(resizeFrame); flushInput(); unsubscribe(); observer?.disconnect(); input?.dispose(); instance.dispose(); terminal.current = null; };
    });
    return () => { disposed = true; cleanup(); };
  }, [interactive, runId, subscribe]);

  return <div aria-label={interactive ? "Interactive agent terminal" : "Agent terminal log"} className="h-full min-h-0 w-full overflow-hidden bg-app p-2" onPointerDown={interactive ? () => terminal.current?.focus() : undefined} ref={host} />;
}
