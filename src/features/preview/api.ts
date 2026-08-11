import { invoke } from "@tauri-apps/api/core";
import type { Preview, PreviewLogChunk } from "./model";

export const preparePreview = (attemptId: string, fingerprint: string, runId: string | null) => invoke<Preview>("preview_prepare", { input: { attemptId, fingerprint, runId } });
export const getPreview = (previewId: string) => invoke<Preview>("preview_get", { input: { previewId } });
export const startPreview = (previewId: string, commandFingerprint: string) => invoke<Preview>("preview_start", { input: { previewId, commandFingerprint } });
export const stopPreview = (previewId: string) => invoke<Preview>("preview_stop", { input: { previewId } });
export const restartPreview = (previewId: string) => invoke<Preview>("preview_restart", { input: { previewId } });
export const closePreview = (previewId: string) => invoke<void>("preview_close", { input: { previewId } });
export const readPreviewLog = (previewId: string, cursor: number) => invoke<PreviewLogChunk>("preview_read_log", { input: { previewId, cursor } });
