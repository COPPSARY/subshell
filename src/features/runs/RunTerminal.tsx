import { useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { resizeRun, writeRunInput } from "./api";

export function RunTerminal({ runId, chunks }: { runId: string; chunks: Uint8Array[] }) {
  const host = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | null>(null);
  const written = useRef(0);
  const pendingChunks = useRef(chunks);
  pendingChunks.current = chunks;

  useEffect(() => {
    let disposed = false;
    let cleanup = () => undefined;
    void Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit")]).then(([xterm, addon]) => {
      if (disposed || !host.current) return;
      const instance = new xterm.Terminal({ convertEol: true, cursorBlink: true, fontFamily: "ui-monospace, SFMono-Regular, monospace", fontSize: 13, screenReaderMode: true, theme: { background: "#08090b", foreground: "#d9dce2", cursor: "#849aff" } });
      const fit = new addon.FitAddon(); instance.loadAddon(fit); instance.open(host.current); fit.fit(); terminal.current = instance;
      for (const chunk of pendingChunks.current) instance.write(chunk);
      written.current = pendingChunks.current.length;
      const input = instance.onData((data) => writeRunInput(runId, Array.from(new TextEncoder().encode(data))).catch(() => undefined));
      const resize = () => { fit.fit(); resizeRun(runId, instance.rows, instance.cols).catch(() => undefined); };
      const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(resize); observer?.observe(host.current);
      cleanup = () => { observer?.disconnect(); input.dispose(); instance.dispose(); terminal.current = null; };
    });
    return () => { disposed = true; cleanup(); };
  }, [runId]);

  useEffect(() => { const instance = terminal.current; if (!instance) return; for (const chunk of chunks.slice(written.current)) instance.write(chunk); written.current = chunks.length; }, [chunks]);
  return <div aria-label="Live agent terminal" className="h-64 w-full overflow-hidden rounded-b-md bg-app p-2" ref={host} />;
}
