import { useEffect, useState } from "react";
import appIconUrl from "../../assets/app-icon.svg";
import { getHealth } from "../features/health";
import { ProjectsView, restoreProject, type Project } from "../features/projects";
import { ProvidersView } from "../features/providers";
import { AgentsView } from "../features/runs";
import { createTask, getTask, listTasks, TasksView, type Task } from "../features/tasks";
import { TimelineView } from "../features/timeline";
import { destinations, type DestinationName } from "./navigation";
import { WorkspaceRail } from "./WorkspaceRail";

export function AppShell() {
  const [active, setActive] = useState<DestinationName>("Projects");
  const [healthFailed, setHealthFailed] = useState(false);
  const [project, setProject] = useState<Project | null>(null);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [autoStartTaskId, setAutoStartTaskId] = useState<string | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);

  useEffect(() => {
    let mounted = true;
    getHealth()
      .then(() => undefined)
      .catch(() => mounted && setHealthFailed(true));
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => { restoreProject().then(setProject).catch(() => undefined); }, []);
  useEffect(() => { if (project) listTasks(project.id).then(setTasks).catch(() => setTasks([])); else setTasks([]); }, [project?.id, active]);

  async function startGoal(selectedProject: Project, goal: string, confirmDirtyBase: boolean) {
    const task = await createTask({
      projectId: selectedProject.id,
      title: goal.split("\n", 1)[0].slice(0, 72),
      description: goal,
      acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [],
      confirmDirtyBase,
    });
    setProject(selectedProject); setSelectedTask(task); setAutoStartTaskId(task.id); setSelectedRunId(null); setActive("Tasks");
  }
  const activeView = active === "Projects"
    ? <ProjectsView selectedProject={project} onSelect={(nextProject) => { if (nextProject.id !== project?.id) { setSelectedTask(null); setSelectedRunId(null); setAutoStartTaskId(null); } setProject(nextProject); }} onStartGoal={startGoal} />
    : active === "Tasks"
      ? <TasksView autoStartTaskId={autoStartTaskId} initialRunId={selectedRunId} initialTask={selectedTask} onAutoStartConsumed={() => setAutoStartTaskId(null)} onSelectRun={setSelectedRunId} onSelectTask={setSelectedTask} project={project} />
      : active === "Agents"
        ? <AgentsView onNewGoal={() => setActive("Projects")} onOpen={(task, runId) => { setSelectedTask(task); setSelectedRunId(runId); setActive("Tasks"); }} project={project} tasks={tasks} />
      : active === "Providers"
        ? <ProvidersView />
        : <TimelineView onOpen={(taskId, runId) => getTask(taskId).then((task) => { setSelectedTask(task); setSelectedRunId(runId ?? null); setActive("Tasks"); })} project={project} tasks={tasks} />;

  return (
    <div className="grid h-full overflow-hidden grid-cols-[14rem_minmax(0,1fr)] bg-app xl:grid-cols-[15rem_minmax(0,1fr)]">
      <aside className="flex min-h-0 flex-col border-r border-line bg-chrome">
        <div className="flex h-12 items-center gap-3 border-b border-line px-4">
          <img alt="" aria-hidden="true" className="size-7" src={appIconUrl} />
          <div className="grid gap-0.5">
            <strong className="text-sm tracking-wide text-primary">SubShell</strong>
            <span className="text-[11px] text-tertiary">Agent coordination</span>
          </div>
        </div>

        <WorkspaceRail onArchived={(taskId) => { if (selectedTask?.id === taskId) { setSelectedTask(null); setSelectedRunId(null); setAutoStartTaskId(null); } }} onSelect={(task, runId) => { setSelectedTask(task); setSelectedRunId(runId ?? null); setAutoStartTaskId(null); setActive("Tasks"); }} project={project} selectedRunId={selectedRunId} selectedTaskId={selectedTask?.id} />

        {healthFailed && <div aria-live="polite" className="mt-auto flex min-h-11 items-center gap-2 border-t border-line px-4 font-mono text-[11px] text-failed" role="status"><span aria-hidden="true" className="size-1.5 rounded-full bg-failed" />Backend unavailable</div>}
      </aside>

      <main className="min-h-0 min-w-0 bg-app">
        <header className="flex h-12 items-center border-b border-line bg-chrome px-4 text-xs text-tertiary">
          <div className="flex items-center gap-2">
            <span>{project?.name ?? "SubShell"}</span><span aria-hidden="true">/</span><strong className="font-medium text-primary">{active}</strong>
          </div>
          <nav className="ml-auto flex items-center gap-1" aria-label="Primary navigation">
            {destinations.map((destination) => {
              const Icon = destination.icon;
              return <button aria-label={destination.name} aria-current={destination.name === active ? "page" : undefined} className="group flex h-8 items-center gap-2 rounded-md px-2.5 text-xs text-secondary hover:bg-panel hover:text-primary focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent aria-[current=page]:bg-selected aria-[current=page]:text-primary" key={destination.name} onClick={() => setActive(destination.name)} title={destination.name} type="button"><Icon aria-hidden="true" className="group-aria-[current=page]:text-accent" size={14} strokeWidth={1.6} /><span className="hidden xl:inline">{destination.name}</span></button>;
            })}
          </nav>
        </header>
        <section className="h-[calc(100vh-3rem)] min-h-0 overflow-auto" aria-live="polite">
          {activeView}
        </section>
      </main>
    </div>
  );
}
