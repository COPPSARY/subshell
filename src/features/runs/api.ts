import { Channel, invoke } from "@tauri-apps/api/core";
import type { Run, RunDiff, RunEvent, TaskPlan } from "./model";

export type RunAssignment = { providerId: string; instruction: string; role?: string; title?: string; contextToken: string; approvedContext: string; environmentFiles: string[] };
export function startRuns(taskId: string, assignments: RunAssignment[], onEvent: (event: RunEvent) => void) {
  const channel = new Channel<RunEvent>(onEvent);
  return invoke<Run[]>("runs_start", { input: { taskId, assignments }, onEvent: channel });
}
export const enqueueRuns = (taskId: string, assignments: RunAssignment[]) => invoke<Run[]>("runs_enqueue", { input: { taskId, assignments } });
export function resumeRun(runId: string, onEvent: (event: RunEvent) => void) {
  const channel = new Channel<RunEvent>(onEvent);
  return invoke<Run>("runs_resume", { input: { runId }, onEvent: channel });
}
export const listRuns = (taskId: string) => invoke<{ items: Run[] }>("runs_list", { input: { taskId } }).then((page) => page.items);
export const getTaskPlan = (taskId: string) => invoke<TaskPlan | null>("runs_plan_get", { input: { taskId } });
export function approveTaskPlan(planId: string, fullAccess: boolean, onEvent: (event: RunEvent) => void) {
  const channel = new Channel<RunEvent>(onEvent);
  return invoke<TaskPlan>("runs_plan_approve", { input: { planId, fullAccess }, onEvent: channel });
}
export const rejectTaskPlan = (planId: string) => invoke<TaskPlan>("runs_plan_reject", { input: { planId } });
export const readRunOutput = (runId: string, cursor = 0) => invoke<{ bytes: number[]; nextCursor: number }>("runs_read_output", { input: { runId, cursor, limit: 65536 } });
export const readRunOutputTail = (runId: string) => invoke<{ bytes: number[]; nextCursor: number }>("runs_read_output", { input: { runId, cursor: 0, limit: 65536, tail: true } });
export const writeRunInput = (runId: string, bytes: number[]) => invoke<void>("runs_write_input", { input: { runId, bytes } });
export const resizeRun = (runId: string, rows: number, cols: number) => invoke<void>("runs_resize", { input: { runId, rows, cols } });
export const stopRun = (runId: string) => invoke<void>("runs_stop", { input: { runId } });
export const completeRun = (runId: string) => invoke<Run>("runs_mark_complete", { input: { runId } });
export const readRunDiff = (runId: string) => invoke<RunDiff>("runs_diff", { input: { runId } });
export const previewRunEnvironment = (projectId: string, files: string[]) => invoke<{ files: string[]; port: null }>("runs_environment_preview", { input: { projectId, files } });
