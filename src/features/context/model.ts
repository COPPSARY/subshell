export type ContextEntry = { source: string; bytes: number; included: boolean; reason: string | null };
export type ContextPreview = { token: string; content: string; sha256: string; manifest: { entries: ContextEntry[]; totalBytes: number; budgetBytes: number; reportedTokens: number | null; wasEdited: boolean; sha256: string } };
