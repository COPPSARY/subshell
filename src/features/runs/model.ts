export type Run = { id: string; taskId: string; providerId: string; providerName: string; instruction: string; role?: string; title?: string | null; status: string; waitingReason?: string | null; worktreePath: string | null; rawLogPath: string | null; contextPackPath: string | null; providerSessionId?: string | null; canResume?: boolean; resumeCount?: number; fullAccess?: boolean; port: number | null; updatedAt: string };
export type RunDiff = { files: string[]; patch: string; truncated: boolean };
export type RunOutputChunk = { bytes: number[]; cursor: number };
export type RunEvent = { type: "started"; runId: string } | { type: "output"; runId: string; bytes: number[]; cursor: number } | { type: "statusChanged"; runId: string; status: string } | { type: "failed"; runId: string; error: { message: string } };
export type TaskPlanAssignment = { id: string; title: string; instruction: string; role: string; allowedPaths: string[]; position: number };
export type TaskPlan = { id: string; taskId: string; plannerRunId: string; summary: string; status: "proposed" | "launched" | "rejected"; assignments: TaskPlanAssignment[]; createdAt: string };
