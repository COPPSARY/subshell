import { invoke } from "@tauri-apps/api/core";
import type { ContextPreview } from "./model";
export const listContextSources = (projectId: string) => invoke<string[]>("context_sources", { input: { projectId } });
export const previewContext = (input: { taskId: string; instruction: string; selectedFiles: string[]; pattern: string | null }) => invoke<ContextPreview>("context_preview", { input });
