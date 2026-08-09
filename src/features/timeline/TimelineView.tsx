import { Activity } from "lucide-react";

export function TimelineView() {
  return (
    <div className="w-full p-7">
      <h1 className="sr-only">Timeline</h1>
      <div className="flex h-11 items-center justify-between border-b border-line">
        <h2 className="text-[15px] font-medium text-primary">All activity</h2>
        <span className="font-mono text-[11px] uppercase tracking-wider text-tertiary">Newest first</span>
      </div>
      <div className="relative min-h-64 border-b border-line py-16 pl-11">
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
      </div>
    </div>
  );
}
