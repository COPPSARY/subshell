import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { listRuns } from "./api";
import { AgentsView } from "./AgentsView";

vi.mock("./api", () => ({ listRuns: vi.fn() }));
vi.mock("../providers", () => ({ ProviderIcon: () => <span /> }));

it("shows active agents across tasks and opens their workspace", async () => {
  const task = { id: "task", projectId: "project", title: "Fix login", description: "", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" };
  const run = { id: "run", taskId: "task", providerId: "codex", providerName: "Codex", instruction: "Fix it", status: "running", worktreePath: "/tmp/run", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" };
  vi.mocked(listRuns).mockResolvedValue([run]);
  const open = vi.fn();

  render(<AgentsView onNewGoal={vi.fn()} onOpen={open} project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} tasks={[task]} />);

  fireEvent.click(await screen.findByRole("button", { name: /Lead · Codex/ }));
  expect(open).toHaveBeenCalledWith(task, "run");
});
