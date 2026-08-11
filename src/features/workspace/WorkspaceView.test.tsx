import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { listProviders } from "../providers";
import { stopRun } from "../runs";
import { createSnapshot } from "./api";
import { getDashboard, listAgentTemplates, listBookmarks, listEnvironmentProfiles, listExplorerAgents, listMergeQueue, listSnapshots } from "./api";
import { WorkspaceView } from "./WorkspaceView";

vi.mock("../providers", () => ({ listProviders: vi.fn() }));
vi.mock("../runs", () => ({ stopRun: vi.fn() }));
vi.mock("./api", () => ({
  createSnapshot: vi.fn(), getDashboard: vi.fn(), listAgentTemplates: vi.fn(), listBookmarks: vi.fn(), listEnvironmentProfiles: vi.fn(), listExplorerAgents: vi.fn(), listMergeQueue: vi.fn(), listSnapshots: vi.fn(), processMergeQueue: vi.fn(), removeAgentTemplate: vi.fn(), removeEnvironmentProfile: vi.fn(), rollbackSnapshot: vi.fn(), saveAgentTemplate: vi.fn(), saveEnvironmentProfile: vi.fn(), searchWorkspace: vi.fn(), toggleBookmark: vi.fn(),
}));

it("shows grouped agent resources and a focused setup screen", async () => {
  vi.mocked(getDashboard).mockResolvedValue({ activeAgents: 1, pendingTasks: 1, blockedTasks: 0, reviews: 0, failures: 0, queuedMerges: 0, snapshots: 0, bookmarks: 0, totalReportedUnits: 16, attentionItems: 1, recentActivity: [{ eventType: "run.started", taskId: "task", createdAt: "now" }] });
  vi.mocked(listAgentTemplates).mockResolvedValue([]);
  vi.mocked(listEnvironmentProfiles).mockResolvedValue([]);
  vi.mocked(listProviders).mockResolvedValue([]);
  vi.mocked(listBookmarks).mockResolvedValue([]);
  vi.mocked(listSnapshots).mockResolvedValue([]);
  vi.mocked(listMergeQueue).mockResolvedValue([]);
  vi.mocked(listExplorerAgents).mockResolvedValue([{ run: { id: "run", taskId: "task", providerName: "Codex", title: "Implement", role: "implementer", status: "running", worktreePath: "/tmp/worktree", port: 4310, canResume: true, unitLimit: 50 }, usage: { active: true, processId: 123, cpuPercent: 4.5, residentBytes: 1048576 } }]);
  const task = { id: "task", projectId: "project", title: "Build preview", description: "", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" };
  render(<WorkspaceView onOpen={vi.fn()} project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} tasks={[task]} />);

  expect(await screen.findByText("Implement")).toBeTruthy();
  expect(screen.getByText("Codex · implementer")).toBeTruthy();
  expect(screen.getByText("123")).toBeTruthy();
  expect(screen.getByText("4310")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Checkpoint and pause agent" }));
  await waitFor(() => expect(createSnapshot).toHaveBeenCalledWith("run", "checkpoint", "Checkpoint · Implement"));
  await waitFor(() => expect(stopRun).toHaveBeenCalledWith("run"));
  fireEvent.click(screen.getByRole("button", { name: "Agent setup" }));
  expect(screen.getByRole("heading", { name: "Agent templates" })).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Environment profiles" })).toBeTruthy();
  expect(screen.queryByText("Usage limits")).toBeNull();
});
