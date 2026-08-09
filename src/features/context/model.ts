export type ContextEntry = { source: string; bytes: number; included: boolean; reason: string | null };
export type ContextPreview = { token: string; content: string; sha256: string; manifest: { entries: ContextEntry[]; totalBytes: number; budgetBytes: number; reportedTokens: number | null; wasEdited: boolean; sha256: string } };
export type SharePreview = { content: string; sha256: string; sizeBytes: number };
export type ContextShare = { id: string; taskId: string; sourceRunId: string | null; targetRunId: string; kind: "file" | "output_excerpt" | "summary"; contentReference: string | null; contentSummary: string; deliveryStatus: "pending" | "delivered" | "failed"; previewSha256: string; sizeBytes: number; deliveryError: string | null; createdAt: string };
