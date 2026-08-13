import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { createProvider, detectProviders, getDefaultProvider, listProviders, loginCodex, reauthenticateProvider, setDefaultProvider } from "./api";
import { ProvidersView } from "./ProvidersView";

vi.mock("./api", () => ({ createProvider: vi.fn(), detectProviders: vi.fn(), getDefaultProvider: vi.fn(), listProviders: vi.fn(), loginCodex: vi.fn(), logoutCodex: vi.fn(), reauthenticateProvider: vi.fn(), removeProvider: vi.fn(), setDefaultProvider: vi.fn(), stopCodexLogin: vi.fn(), updateProvider: vi.fn() }));

beforeEach(() => {
  vi.mocked(listProviders).mockResolvedValue([]);
  vi.mocked(getDefaultProvider).mockResolvedValue(null);
  vi.mocked(setDefaultProvider).mockImplementation(async (id) => id);
  vi.mocked(detectProviders).mockResolvedValue([{ key: "claude", displayName: "Claude Code", executablePath: "/usr/bin/claude", arguments: ["--session-id", "{sessionId}", "{prompt}"], resumeArguments: ["--resume", "{sessionId}"], promptMode: "argument", configRootEnvVar: "CLAUDE_CONFIG_DIR", authProbeArguments: ["auth", "status"], capabilities: { nativeSkills: false, reportsUsage: false, interactiveInput: true }, isConfigured: false, isAuthenticated: false }]);
  vi.mocked(createProvider).mockResolvedValue({ id: "profile-1", displayName: "Claude Code", executablePath: "/usr/bin/claude", arguments: ["--session-id", "{sessionId}", "{prompt}"], resumeArguments: ["--resume", "{sessionId}"], promptMode: "argument", configRootEnvVar: "CLAUDE_CONFIG_DIR", configSourcePath: null, inheritUserHome: true });
});

it("configures a detected CLI with one explicit click", async () => {
  render(<ProvidersView />);
  fireEvent.click(await screen.findByRole("button", { name: "Configure" }));
  expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ displayName: "Claude Code", configRootEnvVar: "CLAUDE_CONFIG_DIR", inheritUserHome: true }));
  expect(await screen.findByText("Using existing CLI login")).toBeTruthy();
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

it("automatically imports an authenticated Codex CLI account", async () => {
  const detected = { key: "codex", displayName: "Codex", executablePath: "C:\\Users\\test\\AppData\\Roaming\\npm\\codex.cmd", arguments: ["{prompt}"], resumeArguments: ["resume", "--last"], promptMode: "argument" as const, configRootEnvVar: "CODEX_HOME", authProbeArguments: ["login", "status"], capabilities: { nativeSkills: false, reportsUsage: false, interactiveInput: true }, isConfigured: false, isAuthenticated: true };
  const account = { id: "codex-existing", displayName: "Codex", providerType: "codex", status: "active" as const, executablePath: detected.executablePath, arguments: detected.arguments, resumeArguments: detected.resumeArguments, promptMode: detected.promptMode, configRootEnvVar: detected.configRootEnvVar, configSourcePath: null, inheritUserHome: true };
  vi.mocked(detectProviders).mockResolvedValue([detected]);
  vi.mocked(createProvider).mockResolvedValue(account);

  render(<ProvidersView />);

  await waitFor(() => expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ providerType: "codex", inheritUserHome: true })));
  expect(await screen.findByText("ChatGPT · Existing Codex home")).toBeTruthy();
  expect(screen.getByText("Existing login detected")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();
});

it("links multiple Codex accounts through the device-code flow", async () => {
  const detected = { key: "codex", displayName: "Codex", executablePath: "/usr/bin/codex", arguments: ["{prompt}"], resumeArguments: ["resume", "--last"], promptMode: "argument" as const, configRootEnvVar: "CODEX_HOME", authProbeArguments: ["login", "status"], capabilities: { nativeSkills: false, reportsUsage: false, interactiveInput: true }, isConfigured: false, isAuthenticated: false };
  const account = { id: "codex-work", displayName: "Work email", providerType: "codex", status: "needs_reauth" as const, executablePath: "/usr/bin/codex", arguments: ["{prompt}"], resumeArguments: ["resume", "--last"], promptMode: "argument" as const, configRootEnvVar: "CODEX_HOME", configSourcePath: "/data/provider-profiles/codex-work", inheritUserHome: false };
  const linked = { ...account, status: "active" as const };
  vi.mocked(detectProviders).mockResolvedValue([detected]);
  vi.mocked(createProvider).mockResolvedValue(account);
  vi.mocked(listProviders).mockResolvedValueOnce([]).mockResolvedValue([linked]);
  vi.mocked(loginCodex).mockImplementation(async (_id, _method, onEvent) => {
    onEvent({ type: "output", text: "\u001b[90mOpen https://auth.openai.com/codex/device and enter TEST-CODE\u001b[0m" });
  });
  render(<ProvidersView />);
  expect(await screen.findByText("Account linking · Codex only")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Link account" }));
  fireEvent.change(screen.getByLabelText("Account label"), { target: { value: "Work email" } });
  fireEvent.click(screen.getByRole("button", { name: "Use device code" }));
  await waitFor(() => expect(loginCodex).toHaveBeenCalledWith("codex-work", "device", expect.any(Function)));
  expect(await screen.findByRole("link", { name: "Continue to ChatGPT" })).toBeTruthy();
  expect(screen.getByText("TEST-CODE")).toBeTruthy();
  expect(screen.getByText("Sign-in details")).toBeTruthy();
  expect(screen.getByRole("log").textContent).not.toContain("\u001b");
  expect(await screen.findByText("Linked", {}, { timeout: 2_000 })).toBeTruthy();
  expect(screen.getAllByRole("button", { name: "Remove Work email" })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Use for new goals" }));
  await waitFor(() => expect(setDefaultProvider).toHaveBeenCalledWith("codex-work"));
  expect(await screen.findByText("Default")).toBeTruthy();
});
