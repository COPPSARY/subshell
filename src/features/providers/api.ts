import { invoke } from "@tauri-apps/api/core";
import type { DetectedProvider, GenericProfile } from "./model";
export const listProviders = () => invoke<{ items: GenericProfile[] }>("providers_list").then((page) => page.items);
export const createProvider = (input: GenericProfile) => invoke<GenericProfile>("providers_create_generic", { input });
export const updateProvider = (input: GenericProfile) => invoke<GenericProfile>("providers_update_generic", { input });
export const removeProvider = (id: string) => invoke<void>("providers_remove", { input: { id } });
export const reauthenticateProvider = (id: string, secret: string) => invoke<GenericProfile>("providers_reauthenticate", { input: { id, secret } });
export const detectProviders = () => invoke<DetectedProvider[]>("providers_detect");
