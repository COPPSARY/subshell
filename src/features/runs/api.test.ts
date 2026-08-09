import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { approveTaskPlan, completeRun, enqueueRuns, getTaskPlan, readRunDiff, readRunOutputTail, resumeRun, startRuns, writeRunInput } from "./api";

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

it("queues assignments without opening an event channel", async () => {
  vi.mocked(invoke).mockResolvedValueOnce([]);
  const assignments = [{ providerId: "profile-1", instruction: "Later", contextToken: "draft-1", approvedContext: "context", environmentFiles: [] }];
  await enqueueRuns("task-1", assignments);
  expect(invoke).toHaveBeenCalledWith("runs_enqueue", { input: { taskId: "task-1", assignments } });
});

it("writes terminal bytes without converting them to text", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(undefined);
  await writeRunInput("run-1", [0, 255, 10]);
  expect(invoke).toHaveBeenCalledWith("runs_write_input", { input: { runId: "run-1", bytes: [0, 255, 10] } });
});

it("resumes a provider session through a fresh event channel", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({});
  await resumeRun("run-1", vi.fn());
  expect(invoke).toHaveBeenCalledWith("runs_resume", expect.objectContaining({ input: { runId: "run-1" }, onEvent: expect.anything() }));
});

it("marks an ended workspace ready for review", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ status: "succeeded" });
  await completeRun("run-1");
  expect(invoke).toHaveBeenCalledWith("runs_mark_complete", { input: { runId: "run-1" } });
});

it("reads changes from the run worktree", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ files: [], patch: "", truncated: false });
  await readRunDiff("run-1");
  expect(invoke).toHaveBeenCalledWith("runs_diff", { input: { runId: "run-1" } });
});

it("restores only the latest terminal output", async () => {
  vi.mocked(invoke).mockResolvedValueOnce({ bytes: [], nextCursor: 0 });
  await readRunOutputTail("run-1");
  expect(invoke).toHaveBeenCalledWith("runs_read_output", { input: { runId: "run-1", cursor: 0, limit: 65536, tail: true } });
});

it("loads and approves a submitted task plan through explicit contracts", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(null).mockResolvedValueOnce({});
  await getTaskPlan("task-1");
  await approveTaskPlan("plan-1", true, vi.fn());
  expect(invoke).toHaveBeenNthCalledWith(1, "runs_plan_get", { input: { taskId: "task-1" } });
  expect(invoke).toHaveBeenNthCalledWith(2, "runs_plan_approve", expect.objectContaining({ input: { planId: "plan-1", fullAccess: true }, onEvent: expect.anything() }));
});
