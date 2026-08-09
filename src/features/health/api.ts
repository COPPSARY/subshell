import { invoke } from "@tauri-apps/api/core";
import type { Health } from "./model";

export function getHealth(): Promise<Health> {
  return invoke<Health>("health_status");
}

