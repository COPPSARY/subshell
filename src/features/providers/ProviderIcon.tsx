import ClaudeCode from "@lobehub/icons/es/ClaudeCode/components/Mono";
import Codex from "@lobehub/icons/es/Codex/components/Mono";
import GeminiCLI from "@lobehub/icons/es/GeminiCLI/components/Mono";
import { Bot } from "lucide-react";

type Props = { name: string; size?: number; className?: string; "aria-hidden"?: boolean | "true" | "false" };

export function ProviderIcon({ name, size = 16, className, "aria-hidden": decorative }: Props) {
  const hidden = decorative === true || decorative === "true";
  const normalized = name.toLowerCase();
  const Icon = normalized.includes("claude") ? ClaudeCode : normalized.includes("codex") ? Codex : normalized.includes("gemini") ? GeminiCLI : Bot;
  return <span aria-hidden={hidden || undefined} aria-label={hidden ? undefined : `${name} provider`} className={`inline-grid shrink-0 place-items-center ${className ?? ""}`} role={hidden ? undefined : "img"}><Icon aria-hidden="true" size={size} /></span>;
}
