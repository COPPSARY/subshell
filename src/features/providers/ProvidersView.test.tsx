import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { createProvider, detectProviders, listProviders } from "./api";
import { ProvidersView } from "./ProvidersView";

vi.mock("./api", () => ({ createProvider: vi.fn(), detectProviders: vi.fn(), listProviders: vi.fn(), removeProvider: vi.fn(), updateProvider: vi.fn() }));

beforeEach(() => {
  vi.mocked(listProviders).mockResolvedValue([]);
  vi.mocked(detectProviders).mockResolvedValue([{ key: "claude", displayName: "Claude Code", executablePath: "/usr/bin/claude", arguments: ["--session-id", "{sessionId}", "{prompt}"], resumeArguments: ["--resume", "{sessionId}"], promptMode: "argument", isConfigured: false }]);
  vi.mocked(createProvider).mockResolvedValue({ id: "profile-1", displayName: "Claude Code", executablePath: "/usr/bin/claude", arguments: ["--session-id", "{sessionId}", "{prompt}"], resumeArguments: ["--resume", "{sessionId}"], promptMode: "argument", configRootEnvVar: null, configSourcePath: null, inheritUserHome: true });
});

it("configures a detected CLI with one explicit click", async () => {
  render(<ProvidersView />);
  fireEvent.click(await screen.findByRole("button", { name: "Use existing login" }));
  expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ displayName: "Claude Code", inheritUserHome: true }));
  expect(await screen.findByText("Using existing CLI login")).toBeTruthy();
});

it("keeps custom arguments behind the advanced action", async () => {
  render(<ProvidersView />);
  expect(screen.queryByLabelText("Argument 1")).toBeNull();
  fireEvent.click(await screen.findByRole("button", { name: "Custom CLI" }));
  expect(screen.getByLabelText("Argument 1")).toBeTruthy();
});
