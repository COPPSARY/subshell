import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { createProvider, detectProviders, listProviders, reauthenticateProvider } from "./api";
import { ProvidersView } from "./ProvidersView";

vi.mock("./api", () => ({ createProvider: vi.fn(), detectProviders: vi.fn(), listProviders: vi.fn(), reauthenticateProvider: vi.fn(), removeProvider: vi.fn(), updateProvider: vi.fn() }));

beforeEach(() => {
  vi.mocked(listProviders).mockResolvedValue([]);
  vi.mocked(detectProviders).mockResolvedValue([{ key: "claude", displayName: "Claude Code", executablePath: "/usr/bin/claude", arguments: ["--session-id", "{sessionId}", "{prompt}"], resumeArguments: ["--resume", "{sessionId}"], promptMode: "argument", configRootEnvVar: "CLAUDE_CONFIG_DIR", authProbeArguments: ["auth", "status"], capabilities: { nativeSkills: false, reportsUsage: false, interactiveInput: true }, isConfigured: false }]);
  vi.mocked(createProvider).mockResolvedValue({ id: "profile-1", displayName: "Claude Code", executablePath: "/usr/bin/claude", arguments: ["--session-id", "{sessionId}", "{prompt}"], resumeArguments: ["--resume", "{sessionId}"], promptMode: "argument", configRootEnvVar: "CLAUDE_CONFIG_DIR", configSourcePath: null, inheritUserHome: false });
});

it("configures a detected CLI with one explicit click", async () => {
  render(<ProvidersView />);
  fireEvent.click(await screen.findByRole("button", { name: "Configure" }));
  expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ displayName: "Claude Code", configRootEnvVar: "CLAUDE_CONFIG_DIR", inheritUserHome: false }));
  expect(await screen.findByText("Isolated configuration")).toBeTruthy();
});

it("keeps custom arguments behind the advanced action", async () => {
  render(<ProvidersView />);
  expect(screen.queryByLabelText("Argument 1")).toBeNull();
  fireEvent.click(await screen.findByRole("button", { name: "Custom CLI" }));
  expect(screen.getByLabelText("Argument 1")).toBeTruthy();
});

it("reauthenticates an account without retaining its credential", async () => {
  const account = { id: "profile-1", displayName: "Claude Code", providerType: "claude", status: "needs_reauth" as const, executablePath: "/usr/bin/claude", arguments: ["{prompt}"], resumeArguments: [], promptMode: "argument" as const, configRootEnvVar: "CLAUDE_CONFIG_DIR", configSourcePath: null, inheritUserHome: false };
  vi.mocked(listProviders).mockResolvedValue([account]);
  vi.mocked(reauthenticateProvider).mockResolvedValue({ ...account, status: "active" });
  render(<ProvidersView />);
  fireEvent.click(await screen.findByRole("button", { name: "Update credential for Claude Code" }));
  fireEvent.change(screen.getByLabelText("API token"), { target: { value: "secret-marker" } });
  fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
  expect(reauthenticateProvider).toHaveBeenCalledWith("profile-1", "secret-marker");
  expect(await screen.findByText("Ready")).toBeTruthy();
  expect(screen.queryByDisplayValue("secret-marker")).toBeNull();
});
