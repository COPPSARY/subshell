import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { approveReview, getReview } from "./api";
import type { Review } from "./model";
import { ReviewView } from "./ReviewView";

vi.mock("./api", () => ({ getReview: vi.fn(), approveReview: vi.fn(), sendBackReview: vi.fn(), mergeReview: vi.fn() }));

const review: Review = {
  id: "attempt", taskId: "task", attemptNumber: 1, baseRevision: "abcdef123456", fingerprint: "fingerprint123456",
  decision: "pending", combinedDiffPath: "/tmp/combined.diff", combinedPatch: "diff --git a/a.ts b/a.ts",
  runs: [{ runId: "run", title: "API", providerName: "Codex", instruction: "Build API", explanation: "API implemented", files: ["a.ts"], patchSha256: "patch", contextSha256: "context", contextManifest: { totalBytes: 120, entries: [{ source: "AGENTS.md", included: true }] } }],
  conflicts: [{ category: "same_file", runIds: ["a", "b"], evidence: "a.ts" }], validationEvidence: [{ runId: "run", summary: "Tests passed" }],
};

beforeEach(() => { vi.clearAllMocks(); vi.mocked(getReview).mockResolvedValue(review); vi.mocked(approveReview).mockResolvedValue({ ...review, decision: "approved" }); });

it("shows exact combined evidence and approves its fingerprint", async () => {
  render(<ReviewView taskId="task" />);
  expect(await screen.findByText("API implemented")).toBeTruthy();
  expect(screen.getByText("Tests passed")).toBeTruthy();
  expect(screen.getByText(/diff --git/)).toBeTruthy();
  expect(screen.getByLabelText("Feedback").className).toContain("w-full");
  fireEvent.click(screen.getByRole("button", { name: "Approve exact result" }));
  await waitFor(() => expect(approveReview).toHaveBeenCalledWith("attempt", "fingerprint123456"));
  expect(await screen.findByRole("button", { name: "Merge locally" })).toBeTruthy();
});
