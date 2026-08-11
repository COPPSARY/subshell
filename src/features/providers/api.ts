import { Channel, invoke } from "@tauri-apps/api/core";
import type { DetectedProvider, GenericProfile, ProviderAuthEvent } from "./model";
export const listProviders = () => invoke<{ items: GenericProfile[] }>("providers_list").then((page) => page.items);
export const createProvider = (input: GenericProfile) => invoke<GenericProfile>("providers_create_generic", { input });
export const updateProvider = (input: GenericProfile) => invoke<GenericProfile>("providers_update_generic", { input });
export const removeProvider = (id: string) => invoke<void>("providers_remove", { input: { id } });
export const reauthenticateProvider = (id: string, secret: string) => invoke<GenericProfile>("providers_reauthenticate", { input: { id, secret } });
export const detectProviders = () => invoke<DetectedProvider[]>("providers_detect");
export const getDefaultProvider = () => invoke<string | null>("providers_default");
export const setDefaultProvider = (id: string) => invoke<string>("providers_set_default", { input: { id } });
export function loginCodex(id: string, method: "browser" | "device", onEvent: (event: ProviderAuthEvent) => void) {
  return invoke<void>("providers_codex_login", { input: { id, method }, onEvent: new Channel<ProviderAuthEvent>(onEvent) });
}
export const stopCodexLogin = (id: string) => invoke<void>("providers_codex_login_stop", { input: { id } });
export const logoutCodex = (id: string) => invoke<GenericProfile>("providers_codex_logout", { input: { id } });
