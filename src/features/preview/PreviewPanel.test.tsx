import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { Review } from "../review";
import { getPreview, preparePreview, readPreviewLog, startPreview } from "./api";
import type { Preview } from "./model";
import { PreviewPanel } from "./PreviewPanel";

vi.mock("./api", () => ({
  closePreview: vi.fn(),
  getPreview: vi.fn(),
  preparePreview: vi.fn(),
  readPreviewLog: vi.fn(),
  restartPreview: vi.fn(),
  startPreview: vi.fn(),
  stopPreview: vi.fn(),
}));

const review: Review = {
  id: "attempt", taskId: "task", attemptNumber: 1, baseRevision: "base", fingerprint: "review-fingerprint",
  decision: "pending", combinedDiffPath: "/tmp/combined.diff", combinedPatch: "agent patches",
  conflicts: [], validationEvidence: [],
  runs: [
    { runId: "html", title: "HTML", providerName: "Codex", instruction: "HTML", files: ["index.html"], patchSha256: "a", contextSha256: "a", contextManifest: {} },
    { runId: "css", title: "CSS", providerName: "Codex", instruction: "CSS", files: ["styles.css"], patchSha256: "b", contextSha256: "b", contextManifest: {} },
  ],
};
const ready: Preview = {
  id: "preview", attemptId: "attempt", reviewFingerprint: "review-fingerprint", runId: null,
  scopeLabel: "Combined application", status: "ready", url: "http://127.0.0.1:43123", port: 43123,
  command: { display: "PORT=43123 npm run dev", executable: "npm", arguments: ["run", "dev"], environment: [["PORT", "43123"]], workingDirectory: "/tmp/preview", fingerprint: "command-fingerprint" },
  combinedPatch: "exact combined patch", error: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(preparePreview).mockResolvedValue(ready);
  vi.mocked(getPreview).mockResolvedValue(ready);
  vi.mocked(readPreviewLog).mockResolvedValue({ content: "", cursor: 0 });
  vi.mocked(startPreview).mockResolvedValue({ ...ready, status: "starting" });
});

it("shows the exact command before execution and supports an isolated agent scope", async () => {
  const onCombinedPatch = vi.fn();
  render(<PreviewPanel onCombinedPatch={onCombinedPatch} review={review} />);

  fireEvent.click(screen.getByRole("button", { name: "Prepare preview" }));
  expect(await screen.findByText("PORT=43123 npm run dev")).toBeTruthy();
  expect(startPreview).not.toHaveBeenCalled();
  expect(onCombinedPatch).toHaveBeenCalledWith("exact combined patch");

  fireEvent.click(screen.getByRole("button", { name: "Run command" }));
  await waitFor(() => expect(startPreview).toHaveBeenCalledWith("preview", "command-fingerprint"));

  fireEvent.change(screen.getByLabelText("Scope"), { target: { value: "css" } });
  fireEvent.click(screen.getByRole("button", { name: "Prepare preview" }));
  await waitFor(() => expect(preparePreview).toHaveBeenLastCalledWith("attempt", "review-fingerprint", "css"));
});
