import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";

export type AppCommand = { id: string; label: string; detail: string; run: () => void };

export function CommandPalette({ commands, open, onClose }: { commands: AppCommand[]; open: boolean; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return needle ? commands.filter((command) => `${command.label} ${command.detail}`.toLowerCase().includes(needle)) : commands;
  }, [commands, query]);

  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    setQuery(""); setSelected(0);
    requestAnimationFrame(() => input.current?.focus());
    return () => previousFocus.current?.focus();
  }, [open]);
  useEffect(() => { setSelected(0); }, [query]);

  if (!open) return null;
  function choose(command = matches[selected]) { if (!command) return; command.run(); onClose(); }
  return <div className="fixed inset-0 z-50 flex justify-center bg-black/70 p-4 pt-[12vh]" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section aria-label="Command palette" aria-modal="true" className="h-fit w-full max-w-xl overflow-hidden rounded-lg border border-line-strong bg-chrome" onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } else if (event.key === "Tab") { event.preventDefault(); input.current?.focus(); } else if (event.key === "ArrowDown") { event.preventDefault(); setSelected((value) => Math.min(value + 1, matches.length - 1)); } else if (event.key === "ArrowUp") { event.preventDefault(); setSelected((value) => Math.max(value - 1, 0)); } else if (event.key === "Enter") { event.preventDefault(); choose(); } }} role="dialog">
      <label className="flex h-12 items-center gap-3 border-b border-line px-4"><Search aria-hidden="true" className="text-tertiary" size={16} /><span className="sr-only">Search commands</span><input aria-activedescendant={matches[selected] ? `command-${matches[selected].id}` : undefined} aria-autocomplete="list" aria-controls="command-results" aria-expanded="true" autoComplete="off" className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-tertiary" name="command" onChange={(event) => setQuery(event.target.value)} placeholder="Type a command…" ref={input} role="combobox" value={query} /></label>
      <ul className="max-h-80 overflow-auto p-1.5" id="command-results" role="listbox">{matches.length ? matches.map((command, index) => <li aria-selected={index === selected} className="flex min-h-12 w-full cursor-default items-center gap-3 rounded-md px-3 text-left outline-none hover:bg-panel aria-selected:bg-selected" id={`command-${command.id}`} key={command.id} onClick={() => choose(command)} onMouseEnter={() => setSelected(index)} role="option"><span className="min-w-0 flex-1"><strong className="block text-sm font-medium text-primary">{command.label}</strong><span className="block truncate text-xs text-tertiary">{command.detail}</span></span><kbd className="font-mono text-[10px] text-tertiary">↵</kbd></li>) : <li aria-selected="false" className="empty-row" role="option">No matching commands</li>}</ul>
    </section>
  </div>;
}
