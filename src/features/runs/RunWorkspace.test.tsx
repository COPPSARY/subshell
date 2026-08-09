import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { listContextSources, previewContext } from "../context";
import { listProviders } from "../providers";
import { RunWorkspace } from "./RunWorkspace";

vi.mock("../context", () => ({ listContextSources: vi.fn(), previewContext: vi.fn() }));
vi.mock("../providers", () => ({ listProviders: vi.fn() }));
vi.mock("./api", () => ({ previewRunEnvironment: vi.fn(), startRuns: vi.fn(), stopRun: vi.fn() }));

beforeEach(() => {
  vi.mocked(listProviders).mockResolvedValue([{ id: "p1", displayName: "Stand-in", executablePath: "/tmp/agent", arguments: ["{prompt}"], promptMode: "argument", configRootEnvVar: null, configSourcePath: null }]);
  vi.mocked(listContextSources).mockResolvedValue(["README.md"]);
  vi.mocked(previewContext).mockResolvedValue({ token: "draft", content: "focused context", sha256: "abc", manifest: { entries: [{ source: "task", bytes: 15, included: true, reason: null }], totalBytes: 15, budgetBytes: 65536, reportedTokens: null, wasEdited: false, sha256: "abc" } });
});

it("previews editable context and adds independent assignments", async () => {
  render(<RunWorkspace project={{ id: "project", name: "Repo", path: "/tmp/repo", lastOpenedAt: "now", git: { isRepository: true, branch: "main", revision: "abc", dirty: false } }} task={{ id: "task", projectId: "project", title: "Task", description: "", status: "task", baseBranch: "main", baseRevision: "abc", acceptanceCriteria: [], allowedPaths: [], validationCommands: [], decisions: [], updatedAt: "now" }} />);
  expect(await screen.findByRole("option", { name: "Stand-in" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Preview context" }));
  expect(await screen.findByDisplayValue("focused context")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Add assignment" }));
  expect(screen.getByRole("heading", { name: "Assignment 2" })).toBeTruthy();
});
