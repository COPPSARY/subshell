import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Search } from "lucide-react";
import { errorMessage } from "../shared/error";
import appIconUrl from "../../assets/app-icon.svg";
import { getHealth } from "../features/health";
import { listProjects, ProjectsView, restoreProject, type Project } from "../features/projects";
import { ProvidersView } from "../features/providers";
import { approveReview, getReview, mergeReview } from "../features/review";
import { AgentsView, decideQuit } from "../features/runs";
import { createTask, getTask, listTasks, TasksView, updateTaskStatus, type Task } from "../features/tasks";
import { TimelineView } from "../features/timeline";
import { destinations, type DestinationName } from "./navigation";
import { WorkspaceRail } from "./WorkspaceRail";
import { CommandPalette, type AppCommand } from "./CommandPalette";
import { SafeQuitDialog } from "./SafeQuitDialog";
import { WorkspaceView } from "../features/workspace";

export function AppShell() {
  const [active, setActive] = useState<DestinationName>("Projects");
  const [healthFailed, setHealthFailed] = useState(false);
  const [project, setProject] = useState<Project | null>(null);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [autoStartTaskId, setAutoStartTaskId] = useState<string | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [quitActiveRuns, setQuitActiveRuns] = useState<number | null>(null);
  const [commandsOpen, setCommandsOpen] = useState(false);
  const [commandError, setCommandError] = useState("");

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
  useEffect(() => {
    let disposed = false;
    let unlisten: () => void = () => undefined;
    listen<number>("runs-quit-requested", (event) => setQuitActiveRuns(event.payload))
      .then((next) => { if (disposed) next(); else unlisten = next; })
      .catch(() => undefined);
    return () => { disposed = true; unlisten(); };
  }, []);
  useEffect(() => { if (project) listTasks(project.id).then(setTasks).catch(() => setTasks([])); else setTasks([]); }, [project?.id, active]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") { event.preventDefault(); setCommandsOpen((value) => !value); } };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, []);

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
  async function openTask(task: Task, runId?: string | null) {
    if (task.projectId !== project?.id) {
      const owner = (await listProjects()).find((item) => item.id === task.projectId);
      if (owner) setProject(owner);
    }
    setSelectedTask(task); setSelectedRunId(runId ?? null); setActive("Tasks");
  }
  function runCommand(action: () => Promise<void>) { setCommandError(""); void action().catch((reason) => setCommandError(errorMessage(reason))); }
  async function refreshSelected(taskId: string) { const task = await getTask(taskId); setSelectedTask(task); setTasks((items) => items.map((item) => item.id === task.id ? task : item)); setActive("Tasks"); }
  const commands = useMemo<AppCommand[]>(() => {
    const items = destinations.map((destination) => ({ id: `open-${destination.name.toLowerCase()}`, label: `Open ${destination.name}`, detail: destination.name === "Projects" ? "Open a repository or start a goal" : `Go to the ${destination.name.toLowerCase()} workspace`, run: () => setActive(destination.name) }));
    items.unshift({ id: "new-goal", label: "Start a new goal", detail: "Open the quick goal composer", run: () => setActive("Projects") });
    if (!selectedTask) return items;
    items.push({ id: "open-current-task", label: "Open current task", detail: selectedTask.title, run: () => setActive("Tasks") });
    if (selectedTask.status === "review") items.push({ id: "approve-current-review", label: "Approve current review", detail: "Approve the exact assembled diff", run: () => runCommand(async () => { const review = await getReview(selectedTask.id); await approveReview(review.id, review.fingerprint); await refreshSelected(selectedTask.id); }) });
    if (selectedTask.status === "approved") items.push({ id: "merge-current-review", label: "Merge current review", detail: "Run validation and merge the exact approval", run: () => runCommand(async () => { const review = await getReview(selectedTask.id); await mergeReview(review.id, review.fingerprint); await refreshSelected(selectedTask.id); }) });
    items.push({ id: "archive-current-task", label: "Archive current workspace", detail: "Available after all agents stop", run: () => runCommand(async () => { await updateTaskStatus(selectedTask.id, "archived"); setTasks((values) => values.filter((task) => task.id !== selectedTask.id)); setSelectedTask(null); setSelectedRunId(null); setActive("Tasks"); }) });
    return items;
  }, [selectedTask?.id, selectedTask?.status, selectedTask?.title]);
  const activeView = active === "Projects"
    ? <ProjectsView selectedProject={project} onSelect={(nextProject) => { if (nextProject.id !== project?.id) { setSelectedTask(null); setSelectedRunId(null); setAutoStartTaskId(null); } setProject(nextProject); }} onStartGoal={startGoal} />
    : active === "Workspace"
      ? <WorkspaceView onOpen={openTask} onOpenResult={(taskId, runId) => getTask(taskId).then((task) => openTask(task, runId))} project={project} tasks={tasks} />
    : active === "Tasks"
      ? <TasksView autoStartTaskId={autoStartTaskId} initialRunId={selectedRunId} initialTask={selectedTask} onAutoStartConsumed={() => setAutoStartTaskId(null)} onSelectRun={setSelectedRunId} onSelectTask={setSelectedTask} project={project} />
      : active === "Agents"
        ? <AgentsView onNewGoal={() => setActive("Projects")} onOpen={(task, runId) => { setSelectedTask(task); setSelectedRunId(runId); setActive("Tasks"); }} project={project} tasks={tasks} />
      : active === "Providers"
        ? <ProvidersView />
        : <TimelineView onOpen={(taskId, runId) => getTask(taskId).then((task) => openTask(task, runId))} project={project} tasks={tasks} />;

  return (
    <div className="grid h-full overflow-hidden grid-cols-[14rem_minmax(0,1fr)] bg-app xl:grid-cols-[15rem_minmax(0,1fr)]">
      <a className="sr-only z-[60] rounded bg-primary px-3 py-2 text-app focus:not-sr-only focus:fixed focus:left-3 focus:top-3" href="#main-content">Skip to main content</a>
      <aside className="flex min-h-0 flex-col border-r border-line bg-chrome">
        <div className="flex h-12 items-center gap-3 border-b border-line px-4">
          <img alt="" aria-hidden="true" className="size-7" height="28" src={appIconUrl} width="28" />
          <div className="grid gap-0.5">
            <strong className="text-sm tracking-wide text-primary">SubShell</strong>
            <span className="text-[11px] text-tertiary">Agent coordination</span>
          </div>
        </div>

        <WorkspaceRail onArchived={(taskId) => { if (selectedTask?.id === taskId) { setSelectedTask(null); setSelectedRunId(null); setAutoStartTaskId(null); } }} onSelect={(task, runId) => { setSelectedTask(task); setSelectedRunId(runId ?? null); setAutoStartTaskId(null); setActive("Tasks"); }} project={project} selectedRunId={selectedRunId} selectedTaskId={selectedTask?.id} />

        {healthFailed && <div aria-live="polite" className="mt-auto flex min-h-11 items-center gap-2 border-t border-line px-4 font-mono text-[11px] text-failed" role="status"><span aria-hidden="true" className="size-1.5 rounded-full bg-failed" />Backend unavailable</div>}
      </aside>

      <main className="min-h-0 min-w-0 bg-app" id="main-content">
        <header className="flex h-12 items-center border-b border-line bg-chrome px-4 text-xs text-tertiary">
          <div className="flex items-center gap-2">
            <span>{project?.name ?? "SubShell"}</span><span aria-hidden="true">/</span><strong className="font-medium text-primary">{active}</strong>
          </div>
          <nav className="ml-auto flex items-center gap-1" aria-label="Primary navigation">
            <button aria-label="Open command palette" className="group flex h-8 items-center gap-2 rounded-md px-2.5 text-xs text-secondary outline-none hover:bg-panel hover:text-primary focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent" onClick={() => setCommandsOpen(true)} title="Command palette (Ctrl/Cmd+K)" type="button"><Search aria-hidden="true" size={14} /><span className="hidden xl:inline">Commands</span><kbd className="hidden font-mono text-[9px] text-tertiary 2xl:inline">⌘K</kbd></button>
            {destinations.map((destination) => {
              const Icon = destination.icon;
              return <button aria-label={destination.name} aria-current={destination.name === active ? "page" : undefined} className="group flex h-8 items-center gap-2 rounded-md px-2.5 text-xs text-secondary hover:bg-panel hover:text-primary focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent aria-[current=page]:bg-selected aria-[current=page]:text-primary" key={destination.name} onClick={() => setActive(destination.name)} title={destination.name} type="button"><Icon aria-hidden="true" className="group-aria-[current=page]:text-accent" size={14} strokeWidth={1.6} /><span className="hidden xl:inline">{destination.name}</span></button>;
            })}
          </nav>
        </header>
        <section className="h-[calc(100vh-3rem)] min-h-0 overflow-auto" aria-live="polite">
          {commandError && <p className="error-banner fixed bottom-4 right-4 z-40 max-w-md" role="alert">{commandError}</p>}
          {activeView}
        </section>
      </main>
      <CommandPalette commands={commands} onClose={() => setCommandsOpen(false)} open={commandsOpen} />
      {quitActiveRuns != null && <SafeQuitDialog activeRuns={quitActiveRuns} onDecision={(decision) => { if (decision !== "cancel") void decideQuit(decision); setQuitActiveRuns(null); }} />}
    </div>
  );
}
