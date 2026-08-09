import { invoke } from "@tauri-apps/api/core";
import type { GitStatus, Project } from "./model";

export const listProjects = () => invoke<{ items: Project[] }>("projects_list").then((page) => page.items);
export const restoreProject = () => invoke<Project | null>("projects_restore");
export const openProject = (path: string) => invoke<Project>("projects_open", { input: { path } });
export const getProjectStatus = (path: string) => invoke<GitStatus>("projects_status", { input: { path } });
export const listProjectFiles = (projectId: string) => invoke<{ items: string[]; total: number }>("projects_files", { input: { projectId } });
