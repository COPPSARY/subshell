import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { openProject } from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

it("sends a project path through the owned command contract", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({});
  await openProject("/tmp/repository");
  expect(invoke).toHaveBeenCalledWith("projects_open", { input: { path: "/tmp/repository" } });
});
