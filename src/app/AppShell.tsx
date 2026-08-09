import { useEffect, useState } from "react";
import { FolderGit2 } from "lucide-react";
import appIconUrl from "../../assets/app-icon.svg";
import { getHealth, type Health } from "../features/health";
import { ProjectsView, restoreProject, type Project } from "../features/projects";
import { ProvidersView } from "../features/providers";
import { TasksView } from "../features/tasks";
import { TimelineView } from "../features/timeline";
import { destinations, type DestinationName } from "./navigation";

export function AppShell() {
  const [active, setActive] = useState<DestinationName>("Projects");
  const [health, setHealth] = useState<Health | null>(null);
  const [healthFailed, setHealthFailed] = useState(false);
  const [project, setProject] = useState<Project | null>(null);

  useEffect(() => {
    let mounted = true;
    getHealth()
      .then((result) => mounted && setHealth(result))
      .catch(() => mounted && setHealthFailed(true));
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => { restoreProject().then(setProject).catch(() => undefined); }, []);

  const selected = destinations.find((destination) => destination.name === active)!;
  const activeView = active === "Projects"
    ? <ProjectsView onSelect={setProject} />
    : active === "Tasks"
      ? <TasksView project={project} />
      : active === "Providers"
        ? <ProvidersView />
        : <TimelineView />;

  return (
    <div className="grid min-h-full grid-cols-[236px_minmax(0,1fr)] bg-app">
      <aside className="flex min-h-screen flex-col border-r border-line bg-chrome">
        <div className="flex h-16 items-center gap-3 border-b border-line px-4">
          <img alt="" aria-hidden="true" className="size-8" src={appIconUrl} />
          <div className="grid gap-0.5">
            <strong className="text-sm tracking-wide text-primary">SubShell</strong>
            <span className="text-[11px] text-tertiary">Agent coordination</span>
          </div>
        </div>

        <div className="px-3.5 pb-3 pt-5" aria-label="Current project">
          <SectionLabel>Current project</SectionLabel>
          <div className="flex h-10 items-center gap-2 rounded-md border border-line bg-panel px-2.5 text-xs text-secondary">
            <FolderGit2 aria-hidden="true" size={14} strokeWidth={1.6} />
            <span className="truncate">{project?.name ?? "No project open"}</span>
          </div>
        </div>

        <nav className="px-2 py-3" aria-label="Primary navigation">
          <SectionLabel>Workspace</SectionLabel>
          {destinations.map((destination) => {
            const Icon = destination.icon;
            return (
              <button
                aria-label={destination.name}
                aria-current={destination.name === active ? "page" : undefined}
                className="group flex h-10 w-full items-center gap-2.5 rounded-md border border-transparent px-2.5 text-left text-sm text-secondary transition-colors duration-100 hover:bg-[#101216] hover:text-primary focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent aria-[current=page]:bg-[#14161a] aria-[current=page]:text-primary"
                key={destination.name}
                onClick={() => setActive(destination.name)}
                type="button"
              >
                <Icon className="text-secondary group-aria-[current=page]:text-accent" aria-hidden="true" size={16} strokeWidth={1.6} />
                {destination.name}
                {destination.name === "Tasks" && <span className="ml-auto font-mono text-[10px] text-tertiary">0</span>}
              </button>
            );
          })}
        </nav>

        <div
          aria-live="polite"
          className="mt-auto flex min-h-11 items-center gap-2 border-t border-line px-4 font-mono text-[11px] text-tertiary"
          role="status"
        >
          <span aria-hidden="true" className={`size-1.5 rounded-full ${healthFailed ? "bg-failed" : "bg-complete"}`} />
          {health
            ? `Schema ${health.schemaVersion} · Ready`
            : healthFailed
              ? "Backend unavailable"
              : "Connecting to backend…"}
        </div>
      </aside>

      <main className="min-w-0 bg-app">
        <header className="flex h-16 items-center border-b border-line bg-chrome px-5 text-xs text-tertiary">
          <div className="flex items-center gap-2">
            <span>SubShell</span><span aria-hidden="true">/</span><strong className="font-medium text-primary">{active}</strong>
          </div>
        </header>
        <section className="min-h-[calc(100vh-4rem)]" aria-live="polite">
          {activeView}
        </section>
      </main>
    </div>
  );
}

function SectionLabel({ children }: { children: string }) {
  return <span className="mx-2 mb-2 block text-[11px] font-bold uppercase tracking-[0.12em] text-tertiary">{children}</span>;
}
