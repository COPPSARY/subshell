import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { listRuns } from "../features/runs";
import { listArchivedTasks, listTasks, updateTaskStatus } from "../features/tasks";
import { WorkspaceRail } from "./WorkspaceRail";

vi.mock("../features/runs", () => ({ listRuns: vi.fn() }));
vi.mock("../features/tasks", () => ({ listArchivedTasks: vi.fn(), listTasks: vi.fn(), updateTaskStatus: vi.fn() }));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listArchivedTasks).mockResolvedValue([]);
});

it("lists task workspaces and opens their agent sessions", async () => {
  const task = { id: "task-1", projectId: "project-1", title: "Fix login", description: "", status: "active", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" };
  vi.mocked(listTasks).mockResolvedValue([task]);
  vi.mocked(listRuns).mockResolvedValue([{ id: "run-1", taskId: task.id, providerId: "codex", providerName: "Codex", instruction: "Fix login", status: "running", worktreePath: "/tmp/worktree", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]);
  const select = vi.fn();

  render(<WorkspaceRail onSelect={select} project={{ id: "project-1", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} />);

  fireEvent.click(await screen.findByRole("button", { name: /Codex/ }));
  expect(select).toHaveBeenCalledWith(task, "run-1");
});

it("marks only the selected run instead of also selecting its parent task", async () => {
  const task = { id: "task-1", projectId: "project-1", title: "Fix login", description: "", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" };
  vi.mocked(listTasks).mockResolvedValue([task]);
  vi.mocked(listRuns).mockResolvedValue([{ id: "run-1", taskId: task.id, providerId: "codex", providerName: "Codex", instruction: "Fix login", status: "running", worktreePath: "/tmp/worktree", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]);

  render(<WorkspaceRail onSelect={vi.fn()} project={{ id: "project-1", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} selectedRunId="run-1" selectedTaskId="task-1" />);

  const taskButton = await screen.findByRole("button", { name: /Fix login/ });
  const runButton = screen.getByRole("button", { name: /Codex/ });
  expect(taskButton.getAttribute("aria-current")).toBeNull();
  expect(runButton.getAttribute("aria-current")).toBe("page");
});

it("archives a completed workspace even when its task status is stale", async () => {
  const task = { id: "task-1", projectId: "project-1", title: "Fix login", description: "", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" };
  vi.mocked(listTasks).mockResolvedValue([task]);
  vi.mocked(listRuns).mockResolvedValue([{ id: "run-1", taskId: task.id, providerId: "codex", providerName: "Codex", instruction: "Fix login", status: "succeeded", worktreePath: "/tmp/worktree", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]);
  vi.mocked(updateTaskStatus).mockResolvedValue({ ...task, status: "archived" });
  const archived = vi.fn();

  render(<WorkspaceRail onArchived={archived} onSelect={vi.fn()} project={{ id: "project-1", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} />);

  fireEvent.click(await screen.findByRole("button", { name: "Archive workspace Fix login" }));
  await waitFor(() => expect(updateTaskStatus).toHaveBeenCalledWith(task.id, "archived"));
  expect(archived).toHaveBeenCalledWith(task.id);
});

it("restores a workspace from Archive", async () => {
  const task = { id: "task-1", projectId: "project-1", title: "Fix login", description: "", status: "archived", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" };
  vi.mocked(listTasks).mockResolvedValue([]);
  vi.mocked(listArchivedTasks).mockResolvedValue([task]);
  vi.mocked(updateTaskStatus).mockResolvedValue({ ...task, status: "task" });

  render(<WorkspaceRail onSelect={vi.fn()} project={{ id: "project-1", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} />);

  fireEvent.click(await screen.findByRole("button", { name: /Archive/ }));
  fireEvent.click(screen.getByRole("button", { name: "Restore Fix login" }));
  await waitFor(() => expect(updateTaskStatus).toHaveBeenCalledWith(task.id, "task"));
});
