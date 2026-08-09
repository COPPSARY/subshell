import { invoke } from "@tauri-apps/api/core";
import type { ContextPreview, ContextShare, SharePreview } from "./model";
export const listContextSources = (projectId: string) => invoke<string[]>("context_sources", { input: { projectId } });
export const previewContext = (input: { taskId: string; instruction: string; selectedFiles: string[]; pattern: string | null }) => invoke<ContextPreview>("context_preview", { input });
export type ShareInput = { sourceRunId: string | null; targetRunId: string; kind: "file" | "output_excerpt" | "summary"; contentReference: string | null; summary: string };
export const previewContextShare = (input: ShareInput) => invoke<SharePreview>("context_share_preview", { input });
export const deliverContextShare = (input: Omit<ShareInput, "summary"> & { content: string; previewSha256: string }) => invoke<ContextShare>("context_share_deliver", { input });
