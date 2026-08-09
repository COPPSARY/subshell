import { invoke } from "@tauri-apps/api/core";
import type { TimelineEvent } from "./model";

export type TimelineFilters = { taskId?: string | null; runId?: string | null; providerId?: string | null; eventType?: string | null; afterSequence?: number | null; limit?: number };
export const listTimeline = (projectId: string, filters: TimelineFilters = {}) => invoke<{ items: TimelineEvent[] }>("timeline_list", { input: { projectId, taskId: filters.taskId ?? null, runId: filters.runId ?? null, providerId: filters.providerId ?? null, eventType: filters.eventType ?? null, afterSequence: filters.afterSequence ?? null, limit: filters.limit ?? 100 } }).then((page) => page.items);
