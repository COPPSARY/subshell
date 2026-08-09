import { invoke } from "@tauri-apps/api/core";
import type { Task } from "./model";
export type CreateTaskInput = Omit<Task, "id" | "status" | "baseBranch" | "baseRevision" | "updatedAt"> & { confirmDirtyBase: boolean };
export const listTasks = (projectId: string) => invoke<{ items: Task[] }>("tasks_list", { input: { projectId } }).then((page) => page.items);
export const createTask = (input: CreateTaskInput) => invoke<Task>("tasks_create", { input });
