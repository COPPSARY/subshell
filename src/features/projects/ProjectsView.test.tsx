import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { getProjectStatus, listProjectFiles, listProjects } from "./api";
import { ProjectsView } from "./ProjectsView";

vi.mock("./api", () => ({ getProjectStatus: vi.fn(), listProjectFiles: vi.fn(), listProjects: vi.fn(), openProject: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeEach(() => {
  vi.mocked(listProjects).mockResolvedValue([{ id: "project-1", name: "subshell", path: "/tmp/subshell", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }]);
  vi.mocked(listProjectFiles).mockResolvedValue({ items: ["src/main.tsx", "README.md"], total: 2 });
  vi.mocked(getProjectStatus).mockResolvedValue({ isRepository: true, branch: "main", revision: "abc", dirty: false });
});

it("starts from one plain-language goal", async () => {
  const start = vi.fn().mockResolvedValue(undefined);
  render(<ProjectsView onStartGoal={start} />);
  fireEvent.change(await screen.findByLabelText("What do you want the agent to do?"), { target: { value: "Fix the flaky test" } });
  fireEvent.click(screen.getByRole("button", { name: "Start agent" }));
  await waitFor(() => expect(start).toHaveBeenCalledWith(expect.objectContaining({ id: "project-1" }), "Fix the flaky test", false));
});

it("shows files for the selected repository", async () => {
  render(<ProjectsView />);
  expect(await screen.findByText("src/main.tsx")).toBeTruthy();
  expect(screen.getByText("README.md")).toBeTruthy();
  expect(listProjectFiles).toHaveBeenCalledWith("project-1");
});

it("starts from committed HEAD without blocking on a dirty checkout", async () => {
  vi.mocked(listProjects).mockResolvedValueOnce([{ id: "project-1", name: "subshell", path: "/tmp/subshell", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: true } }]);
  const start = vi.fn().mockResolvedValue(undefined);
  render(<ProjectsView onStartGoal={start} />);
  fireEvent.change(await screen.findByLabelText("What do you want the agent to do?"), { target: { value: "Fix it" } });
  fireEvent.click(screen.getByRole("button", { name: "Start agent" }));
  await waitFor(() => expect(start).toHaveBeenCalledWith(expect.anything(), "Fix it", true));
});

it("explains that a newly initialized repository needs its first commit", async () => {
  vi.mocked(listProjects).mockResolvedValueOnce([{ id: "project-1", name: "new-project", path: "/tmp/new-project", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: null, dirty: false } }]);

  render(<ProjectsView onStartGoal={vi.fn()} />);

  expect(await screen.findByText(/repository has no commits yet/i)).toBeTruthy();
  expect(screen.queryByText(/Git is required/i)).toBeNull();
  expect(screen.queryByRole("button", { name: "Start agent" })).toBeNull();
});

it("turns a missing desktop backend into an actionable error", async () => {
  vi.mocked(listProjects).mockRejectedValueOnce(new TypeError("Cannot read properties of undefined (reading 'invoke')"));

  render(<ProjectsView />);

  expect((await screen.findByRole("alert")).textContent).toContain("Restart the desktop app, then try again");
});
