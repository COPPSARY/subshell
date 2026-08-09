import { invoke } from "@tauri-apps/api/core";
import type { ApprovalRequest, AttentionItem } from "./model";

export const listAttention = (projectId: string) => invoke<{ items: AttentionItem[] }>("attention_list", { input: { projectId } }).then((page) => page.items);
export const acknowledgeAttention = (key: string, stateFingerprint: string) => invoke<void>("attention_acknowledge", { input: { key, stateFingerprint } });
export const claimAttentionNotification = (key: string, stateFingerprint: string) => invoke<boolean>("attention_claim_notification", { input: { key, stateFingerprint } });
export const listApprovals = (projectId: string) => invoke<{ items: ApprovalRequest[] }>("workspace_list_approvals", { input: { projectId } }).then((page) => page.items);
export const decideApproval = (requestId: string, decision: "approved" | "denied") => invoke<ApprovalRequest>("workspace_decide_action", { input: { requestId, decision } });
