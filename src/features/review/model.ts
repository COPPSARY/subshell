export type ReviewRun = {
  runId: string;
  title: string;
  providerName: string;
  instruction: string;
  explanation?: string | null;
  files: string[];
  patchSha256: string;
  contextSha256: string;
  contextManifest: { entries?: { source: string; included: boolean }[]; totalBytes?: number };
};

export type ConflictFlag = { category: string; runIds: string[]; evidence: string };
export type ValidationEvidence = { runId: string; summary: string };
export type Review = {
  id: string;
  taskId: string;
  attemptNumber: number;
  baseRevision: string;
  fingerprint: string;
  decision: "pending" | "approved" | "sent_back";
  feedback?: string | null;
  combinedDiffPath: string;
  combinedPatch: string;
  runs: ReviewRun[];
  conflicts: ConflictFlag[];
  validationEvidence: ValidationEvidence[];
};
