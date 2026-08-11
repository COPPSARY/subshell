export type PreviewCommand = {
  display: string;
  executable: string;
  arguments: string[];
  environment: [string, string][];
  workingDirectory: string;
  fingerprint: string;
};

export type Preview = {
  id: string;
  attemptId: string;
  reviewFingerprint: string;
  runId?: string | null;
  scopeLabel: string;
  status: "ready" | "starting" | "running" | "failed" | "stopped";
  url: string;
  port: number;
  command: PreviewCommand;
  combinedPatch: string;
  error?: string | null;
};

export type PreviewLogChunk = { content: string; cursor: number };
