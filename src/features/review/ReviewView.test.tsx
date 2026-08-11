import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { enqueueMerge } from "../workspace";
import { approveReview, getReview } from "./api";
import type { Review } from "./model";
import { ReviewView } from "./ReviewView";

vi.mock("./api", () => ({ getReview: vi.fn(), approveReview: vi.fn(), sendBackReview: vi.fn(), mergeReview: vi.fn() }));
vi.mock("../workspace", () => ({ enqueueMerge: vi.fn() }));

const review: Review = {
  id: "attempt", taskId: "task", attemptNumber: 1, baseRevision: "abcdef123456", fingerprint: "fingerprint123456",
  decision: "pending", combinedDiffPath: "/tmp/combined.diff", combinedPatch: "diff --git a/a.ts b/a.ts",
  runs: [{ runId: "run", title: "API", providerName: "Codex", instruction: "Build API", explanation: "API implemented", files: ["a.ts"], patchSha256: "patch", contextSha256: "context", contextManifest: { totalBytes: 120, entries: [{ source: "AGENTS.md", included: true }] } }],
  conflicts: [{ category: "same_file", runIds: ["a", "b"], evidence: "a.ts" }], validationEvidence: [{ runId: "run", summary: "Tests passed" }],
};

beforeEach(() => { vi.clearAllMocks(); vi.mocked(getReview).mockResolvedValue(review); vi.mocked(approveReview).mockResolvedValue({ ...review, decision: "approved" }); vi.mocked(enqueueMerge).mockResolvedValue({ id: "queue", projectId: "project", taskId: "task", attemptId: "attempt", fingerprint: "fingerprint123456", status: "queued", resultRevision: null, error: null, createdAt: "now" }); });

it("shows exact combined evidence and approves its fingerprint", async () => {
  render(<ReviewView taskId="task" />);
  expect(await screen.findByText("API implemented")).toBeTruthy();
  expect(screen.getByText("Tests passed")).toBeTruthy();
  expect(screen.getByText(/diff --git/)).toBeTruthy();
  expect(screen.getByLabelText("Feedback").className).toContain("w-full");
  fireEvent.click(screen.getByRole("button", { name: "Approve exact result" }));
  await waitFor(() => expect(approveReview).toHaveBeenCalledWith("attempt", "fingerprint123456"));
  expect(await screen.findByRole("button", { name: "Merge locally" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Add to merge queue" }));
  await waitFor(() => expect(enqueueMerge).toHaveBeenCalledWith("attempt", "fingerprint123456"));
});

it("shows structured backend errors instead of object coercion", async () => {
  vi.mocked(getReview).mockRejectedValueOnce({ code: "task_not_reviewable", message: "Task must be ready for review" });

  render(<ReviewView taskId="restored-task" />);

  expect((await screen.findByRole("alert")).textContent).toBe("Task must be ready for review");
});

it("shows an archived review without offering another merge", async () => {
  vi.mocked(getReview).mockResolvedValueOnce({ ...review, decision: "approved" });

  render(<ReviewView readOnly taskId="task" />);

  expect(await screen.findByText("Merged and archived")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Merge locally" })).toBeNull();
});
