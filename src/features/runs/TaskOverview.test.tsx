import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { TaskOverview } from "./TaskOverview";

it("shows real agent state and opens the live terminal", () => {
  const select = vi.fn();
  render(<TaskOverview onSelectRun={select} runs={[{ id: "run-1", taskId: "task", providerId: "codex", providerName: "Codex", instruction: "Implement phase 3", status: "running", worktreePath: "/tmp/run", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]} task={{ id: "task", projectId: "project", title: "Implement phase 3", description: "Build coordination", status: "working", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: ["Activity is visible"], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  expect(screen.getByRole("heading", { name: "Lead · Codex is running" })).toBeTruthy();
  expect(screen.getByText("Activity is visible")).toBeTruthy();
  expect(within(screen.getByRole("list", { name: "Task progress" })).getByText("Implementation").parentElement?.textContent).toContain("active");
  fireEvent.click(screen.getByRole("button", { name: "Open live terminal" }));
  expect(select).toHaveBeenCalledWith("run-1");
});

it("reports failed agent work honestly", () => {
  render(<TaskOverview onSelectRun={() => undefined} runs={[{ id: "run-1", taskId: "task", providerId: "codex", providerName: "Codex", instruction: "Implement", status: "failed", worktreePath: "/tmp/run", rawLogPath: null, contextPackPath: null, port: null, updatedAt: "now" }]} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "failed", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  expect(screen.getByRole("heading", { name: "Lead · Codex is failed" })).toBeTruthy();
  expect(within(screen.getByRole("list", { name: "Task progress" })).getByText("Implementation").parentElement?.textContent).toContain("failed");
});
