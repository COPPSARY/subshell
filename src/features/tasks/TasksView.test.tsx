import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { getTask, listTasks, updateTaskStatus } from "./api";
import type { Task } from "./model";
import { TasksView } from "./TasksView";

vi.mock("./api", () => ({ createTask: vi.fn(), getTask: vi.fn(), listTasks: vi.fn(), updateTaskStatus: vi.fn() }));
vi.mock("../runs", () => ({ RunWorkspace: () => <div>Agent workspace</div> }));

it("groups tasks by lifecycle and opens a selected workspace", async () => {
  const working = task({ id: "active", status: "working", title: "Implement phase 3" });
  vi.mocked(getTask).mockResolvedValue(working);
  vi.mocked(listTasks).mockResolvedValue([working, task({ id: "review", status: "review", title: "Review changes" })]);
  const onSelectTask = vi.fn();

  render(<TasksView onSelectTask={onSelectTask} project={{ id: "project", name: "subshell", path: "/tmp/subshell", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} />);

  const active = await screen.findByRole("region", { name: "Active tasks" });
  expect(within(active).getByRole("button", { name: /Implement phase 3/ })).toBeTruthy();
  expect(within(screen.getByRole("region", { name: "Review tasks" })).getByText("Review changes")).toBeTruthy();
  fireEvent.click(within(active).getByRole("button", { name: /Implement phase 3/ }));
  expect(onSelectTask).toHaveBeenCalledWith(working);
  expect(screen.getByText("Agent workspace")).toBeTruthy();
});

it("persists a task move between lifecycle columns", async () => {
  const working = task({ id: "active", status: "working", title: "Implement phase 3" });
  vi.mocked(listTasks).mockResolvedValue([working]);
  vi.mocked(updateTaskStatus).mockResolvedValue({ ...working, status: "review" });
  render(<TasksView project={{ id: "project", name: "subshell", path: "/tmp/subshell", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} />);

  const active = await screen.findByRole("region", { name: "Active tasks" });
  const card = within(active).getByRole("button", { name: /Implement phase 3/ }).closest("article")!;
  const transfer = { dropEffect: "none", effectAllowed: "none", getData: () => "active", setData: vi.fn() };
  fireEvent.dragStart(card, { dataTransfer: transfer });
  fireEvent.dragOver(screen.getByRole("region", { name: "Review tasks" }), { dataTransfer: transfer });
  fireEvent.drop(screen.getByRole("region", { name: "Review tasks" }), { dataTransfer: transfer });
  expect(updateTaskStatus).toHaveBeenCalledWith("active", "review");
  await waitFor(() => expect(within(screen.getByRole("region", { name: "Review tasks" })).getByText("Implement phase 3")).toBeTruthy());
});

it("shows a structured move error as readable text", async () => {
  const review = task({ id: "review", status: "review", title: "Review changes" });
  vi.mocked(listTasks).mockResolvedValue([review]);
  vi.mocked(updateTaskStatus).mockRejectedValueOnce({ code: "task_has_active_runs", message: "Stop active agents before moving this task", retryable: false });
  render(<TasksView project={{ id: "project", name: "subshell", path: "/tmp/subshell", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} />);

  const reviewColumn = await screen.findByRole("region", { name: "Review tasks" });
  const card = within(reviewColumn).getByRole("button", { name: /Review changes/ }).closest("article")!;
  const transfer = { dropEffect: "none", effectAllowed: "none", getData: () => "review", setData: vi.fn() };
  fireEvent.dragStart(card, { dataTransfer: transfer });
  fireEvent.drop(screen.getByRole("region", { name: "Attention tasks" }), { dataTransfer: transfer });

  expect((await screen.findByRole("alert")).textContent).toBe("Stop active agents before moving this task");
});

function task(values: Partial<Task>): Task {
  return { id: "task", projectId: "project", title: "Task", description: "", status: "task", baseBranch: "main", baseRevision: "49c40ddd", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now", ...values };
}
