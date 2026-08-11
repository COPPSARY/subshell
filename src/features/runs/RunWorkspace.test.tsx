import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { listContextSources, previewContext } from "../context";
import { createProvider, detectProviders, listProviders } from "../providers";
import { approveTaskPlan, completeRun, getTaskPlan, listRuns, readRunDiff, rejectTaskPlan, resumeRun, startRuns } from "./api";
import { RunWorkspace } from "./RunWorkspace";

vi.mock("../context", () => ({ listContextSources: vi.fn(), previewContext: vi.fn() }));
vi.mock("../providers", () => ({ createProvider: vi.fn(), detectProviders: vi.fn(), listProviders: vi.fn(), ProviderIcon: ({ name }: { name: string }) => <span>{name}</span> }));
vi.mock("../review", () => ({ ReviewView: () => <div>Combined review</div> }));
vi.mock("./api", () => ({ approveTaskPlan: vi.fn(), completeRun: vi.fn(), getTaskPlan: vi.fn(), listRuns: vi.fn(), previewRunEnvironment: vi.fn(), readRunDiff: vi.fn(), rejectTaskPlan: vi.fn(), resumeRun: vi.fn(), startRuns: vi.fn(), stopRun: vi.fn() }));
const terminalView = vi.hoisted(() => ({ received: 0, renders: 0 }));
vi.mock("./RunTerminal", () => ({ RunTerminal: ({ runId, subscribe }: { runId: string; subscribe: (id: string, listener: () => void) => () => void }) => { terminalView.renders += 1; subscribe(runId, () => { terminalView.received += 1; }); return <div>Terminal</div>; } }));

beforeEach(() => {
  terminalView.received = 0; terminalView.renders = 0;
  vi.mocked(listProviders).mockResolvedValue([{ id: "p1", displayName: "Stand-in", executablePath: "/tmp/agent", arguments: ["{prompt}"], promptMode: "argument", configRootEnvVar: null, configSourcePath: null, inheritUserHome: false }]);
  vi.mocked(listContextSources).mockResolvedValue(["README.md"]);
  vi.mocked(listRuns).mockResolvedValue([]);
  vi.mocked(getTaskPlan).mockResolvedValue(null);
  vi.mocked(readRunDiff).mockResolvedValue({ files: ["src/app.ts"], patch: "+changed", truncated: false });
  vi.mocked(previewContext).mockResolvedValue({ token: "draft", content: "focused context", sha256: "abc", manifest: { entries: [{ source: "task", bytes: 15, included: true, reason: null }], totalBytes: 15, budgetBytes: 65536, reportedTokens: null, wasEdited: false, sha256: "abc" } });
});

it("surfaces a planner proposal and starts its approved assignments", async () => {
  const planner = { id: "planner", taskId: "task", providerId: "p1", providerName: "Codex", instruction: "Plan", role: "planner", title: "Plan the goal", status: "waiting", waitingReason: "Plan ready for approval", worktreePath: "/tmp/planner", rawLogPath: null, contextPackPath: null, port: 4100, updatedAt: "now" };
  const executor = { ...planner, id: "executor", role: "executor", title: "Build UI", instruction: "Implement the UI", status: "running", waitingReason: null, worktreePath: "/tmp/executor" };
  const plan = { id: "plan", taskId: "task", plannerRunId: "planner", summary: "Build the feature in parallel", status: "proposed" as const, assignments: [{ id: "assignment", title: "Build UI", instruction: "Implement the UI", role: "executor", allowedPaths: ["src/"], position: 0 }], createdAt: "now" };
  vi.mocked(listRuns).mockResolvedValueOnce([planner]).mockResolvedValue([planner, executor]);
  vi.mocked(getTaskPlan).mockResolvedValue(plan);
  vi.mocked(approveTaskPlan).mockResolvedValue({ ...plan, status: "launched" });
  vi.mocked(rejectTaskPlan).mockResolvedValue({ ...plan, status: "rejected" });

  render(<RunWorkspace project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "waiting", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);

  fireEvent.click(await screen.findByRole("button", { name: "Review plan" }));
  expect(screen.getByRole("region", { name: "Plan ready for approval" })).toBeTruthy();
  fireEvent.click(screen.getByRole("checkbox", { name: /Run agents with full permissions/ }));
  fireEvent.click(screen.getByRole("button", { name: "Approve and start" }));
  await waitFor(() => expect(approveTaskPlan).toHaveBeenCalledWith("plan", true, expect.any(Function)));
  await waitFor(() => expect(screen.getByRole("tab", { name: /Build UI/ }).getAttribute("aria-selected")).toBe("true"));
});

it("previews editable context and adds independent assignments", async () => {
  render(<RunWorkspace project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "task", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  expect(await screen.findByRole("option", { name: "Stand-in" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Preview context" }));
  expect(await screen.findByDisplayValue("focused context")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Add assignment" }));
  expect(screen.getByRole("heading", { name: "Assignment 2" })).toBeTruthy();
});

it("turns a quick goal into a context-backed run automatically", async () => {
  const consumed = vi.fn();
  const plannerInstruction = "Inspect this goal and repository, then submit a bounded parallel plan through SubShell. Do not edit files in the planner Run.";
  const planner = { id: "run-1", taskId: "task", providerId: "p1", providerName: "Stand-in", instruction: plannerInstruction, role: "planner", status: "running", worktreePath: "/tmp/worktree", rawLogPath: "/tmp/log", contextPackPath: "/tmp/context", canResume: true, port: 4100, updatedAt: "now" };
  let onRunEvent: Parameters<typeof startRuns>[2] = () => undefined;
  vi.mocked(listRuns).mockResolvedValueOnce([]).mockResolvedValue([planner]);
  vi.mocked(startRuns).mockImplementation(async (_taskId, _assignments, onEvent) => {
    onRunEvent = onEvent;
    return [planner];
  });
  render(<RunWorkspace autoStart onAutoStartConsumed={consumed} project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Fix", description: "Fix the bug", status: "task", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  await waitFor(() => expect(startRuns).toHaveBeenCalled());
  expect(previewContext).toHaveBeenCalledWith({ taskId: "task", instruction: plannerInstruction, selectedFiles: [], pattern: null });
  expect(startRuns).toHaveBeenCalledWith("task", [expect.objectContaining({ role: "planner", title: "Plan the goal" })], expect.any(Function));
  expect(consumed).toHaveBeenCalled();
  expect(await screen.findByRole("tab", { name: /Planner/ })).toBeTruthy();
  expect(screen.getByRole("tab", { name: "Terminal" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy();
  expect(screen.getByText("/tmp/worktree")).toBeTruthy();
  const rendersBeforeOutput = terminalView.renders;
  act(() => onRunEvent({ type: "output", runId: "run-1", bytes: [65], cursor: 1 }));
  expect(terminalView.received).toBe(1);
  expect(terminalView.renders).toBe(rendersBeforeOutput);
  fireEvent.click(screen.getByRole("tab", { name: "Changes" }));
  expect((await screen.findAllByText("src/app.ts")).length).toBeGreaterThan(0);
  expect(readRunDiff).toHaveBeenCalledWith("run-1");
  onRunEvent({ type: "statusChanged", runId: "run-1", status: "cancelled" });
  await waitFor(() => expect(screen.getByRole("tab", { name: "Overview" }).getAttribute("aria-selected")).toBe("true"));
  vi.mocked(resumeRun).mockResolvedValue({ id: "run-1", taskId: "task", providerId: "p1", providerName: "Stand-in", instruction: plannerInstruction, role: "planner", status: "running", worktreePath: "/tmp/worktree", rawLogPath: "/tmp/log", contextPackPath: "/tmp/context", canResume: true, resumeCount: 1, port: 4101, updatedAt: "later" });
  fireEvent.click(screen.getByRole("tab", { name: /Planner/ }));
  fireEvent.click(screen.getByRole("button", { name: "Resume session" }));
  await waitFor(() => expect(resumeRun).toHaveBeenCalledWith("run-1", expect.any(Function)));
});

it("automatically uses an installed CLI for the first prompt", async () => {
  vi.mocked(listProviders).mockResolvedValue([]);
  vi.mocked(detectProviders).mockResolvedValue([{ key: "codex", displayName: "Codex", executablePath: "/usr/bin/codex", arguments: ["{prompt}"], resumeArguments: ["resume", "--last"], promptMode: "argument", isConfigured: false }]);
  vi.mocked(createProvider).mockResolvedValue({ id: "p2", displayName: "Codex", executablePath: "/usr/bin/codex", arguments: ["{prompt}"], promptMode: "argument", configRootEnvVar: null, configSourcePath: null, inheritUserHome: true });
  vi.mocked(startRuns).mockResolvedValue([]);
  render(<RunWorkspace autoStart project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Fix", description: "Fix it", status: "task", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  await waitFor(() => expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ executablePath: "/usr/bin/codex", inheritUserHome: true })));
  await waitFor(() => expect(startRuns).toHaveBeenCalledWith("task", [expect.objectContaining({ providerId: "p2" })], expect.any(Function)));
});

it("opens the session selected from the workspace rail", async () => {
  vi.mocked(listRuns).mockResolvedValue([
    { id: "run-1", taskId: "task", providerId: "p1", providerName: "Claude", instruction: "First", status: "running", worktreePath: "/tmp/one", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" },
    { id: "run-2", taskId: "task", providerId: "p1", providerName: "Codex", instruction: "Second", status: "running", worktreePath: "/tmp/two", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" },
  ]);
  render(<RunWorkspace initialRunId="run-2" project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "task", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  expect(await screen.findByText("/tmp/two")).toBeTruthy();
  expect(screen.getByRole("tab", { name: /Agent 2/ }).getAttribute("aria-selected")).toBe("true");
});

it("opens an existing task on its active agent", async () => {
  vi.mocked(listRuns).mockResolvedValue([{ id: "run-1", taskId: "task", providerId: "p1", providerName: "Codex", instruction: "Implement", status: "running", worktreePath: "/tmp/one", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]);
  render(<RunWorkspace project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task overview", description: "See progress", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  expect((await screen.findByRole("tab", { name: /Lead/ })).getAttribute("aria-selected")).toBe("true");
  expect(screen.getByRole("tab", { name: "Terminal" }).getAttribute("aria-selected")).toBe("true");
});

it("opens review from completed runs even when the selected task status is stale", async () => {
  vi.mocked(listRuns).mockResolvedValue([{ id: "run-1", taskId: "task", providerId: "p1", providerName: "Codex", instruction: "Implement", role: "executor", status: "succeeded", worktreePath: "/tmp/one", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]);
  render(<RunWorkspace project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "old" }} />);
  expect(await screen.findByRole("tab", { name: "Review" })).toBeTruthy();
  expect(screen.getByText("Combined review")).toBeTruthy();
});

it("keeps preserved failed-run changes and advances to review", async () => {
  const failed = { id: "run-1", taskId: "task", providerId: "p1", providerName: "Codex", instruction: "Implement", role: "executor", status: "failed", worktreePath: "/tmp/one", rawLogPath: null, contextPackPath: null, canResume: true, port: null, updatedAt: "now" };
  vi.mocked(listRuns).mockResolvedValueOnce([failed]).mockResolvedValue([{ ...failed, status: "succeeded" }]);
  vi.mocked(completeRun).mockResolvedValue({ ...failed, status: "succeeded" });
  render(<RunWorkspace project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "failed", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  fireEvent.click(await screen.findByRole("button", { name: "Keep changes for review" }));
  await waitFor(() => expect(completeRun).toHaveBeenCalledWith("run-1"));
  expect(await screen.findByText("Combined review")).toBeTruthy();
});
