import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { getProjectStatus, listProjectFiles, openProject } from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

it("sends a project path through the owned command contract", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({});
  await openProject("/tmp/repository");
  expect(invoke).toHaveBeenCalledWith("projects_open", { input: { path: "/tmp/repository" } });
});

it("loads files through the selected project id", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ items: ["src/main.ts"], total: 1 });
  await expect(listProjectFiles("project-1")).resolves.toEqual({ items: ["src/main.ts"], total: 1 });
  expect(invoke).toHaveBeenCalledWith("projects_files", { input: { projectId: "project-1" } });
});

it("refreshes Git status by project path", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ isRepository: true, branch: "main", revision: null, dirty: false });
  await getProjectStatus("/tmp/repository");
  expect(invoke).toHaveBeenCalledWith("projects_status", { input: { path: "/tmp/repository" } });
});
