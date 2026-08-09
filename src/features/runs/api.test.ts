import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { startRuns, writeRunInput } from "./api";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class<T> { onmessage: (event: T) => void; constructor(onmessage: (event: T) => void) { this.onmessage = onmessage; } },
}));

beforeEach(() => vi.mocked(invoke).mockReset());

it("keeps assignments provider-neutral and passes a streaming channel", async () => {
  vi.mocked(invoke).mockResolvedValueOnce([]);
  const assignments = [{ providerId: "profile-1", instruction: "Implement", contextToken: "draft-1", approvedContext: "context", environmentFiles: [] }];
  await startRuns("task-1", assignments, vi.fn());
  expect(invoke).toHaveBeenCalledWith("runs_start", expect.objectContaining({ input: { taskId: "task-1", assignments }, onEvent: expect.anything() }));
});

it("writes terminal bytes without converting them to text", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(undefined);
  await writeRunInput("run-1", [0, 255, 10]);
  expect(invoke).toHaveBeenCalledWith("runs_write_input", { input: { runId: "run-1", bytes: [0, 255, 10] } });
});
