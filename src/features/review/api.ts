import { invoke } from "@tauri-apps/api/core";
import type { Review } from "./model";

export const getReview = (taskId: string) => invoke<Review>("review_get", { input: { taskId } });
export const approveReview = (attemptId: string, fingerprint: string) => invoke<Review>("review_approve", { input: { attemptId, fingerprint, feedback: "" } });
export const sendBackReview = (attemptId: string, fingerprint: string, feedback: string) => invoke<Review>("review_send_back", { input: { attemptId, fingerprint, feedback } });
export const mergeReview = (attemptId: string, fingerprint: string) => invoke<string>("review_merge", { input: { attemptId, fingerprint } });
