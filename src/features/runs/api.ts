import { Channel, invoke } from "@tauri-apps/api/core";
import type { Run, RunEvent } from "./model";

export type RunAssignment = { providerId: string; instruction: string; contextToken: string; approvedContext: string; environmentFiles: string[] };
export function startRuns(taskId: string, assignments: RunAssignment[], onEvent: (event: RunEvent) => void) {
  const channel = new Channel<RunEvent>(onEvent);
  return invoke<Run[]>("runs_start", { input: { taskId, assignments }, onEvent: channel });
}
export const listRuns = (taskId: string) => invoke<{ items: Run[] }>("runs_list", { input: { taskId } }).then((page) => page.items);
export const readRunOutput = (runId: string, cursor = 0) => invoke<{ bytes: number[]; nextCursor: number }>("runs_read_output", { input: { runId, cursor, limit: 65536 } });
export const writeRunInput = (runId: string, bytes: number[]) => invoke<void>("runs_write_input", { input: { runId, bytes } });
export const resizeRun = (runId: string, rows: number, cols: number) => invoke<void>("runs_resize", { input: { runId, rows, cols } });
export const stopRun = (runId: string) => invoke<void>("runs_stop", { input: { runId } });
export const previewRunEnvironment = (projectId: string, files: string[]) => invoke<{ files: string[]; port: null }>("runs_environment_preview", { input: { projectId, files } });
