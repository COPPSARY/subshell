export type GitStatus = { isRepository: boolean; branch: string | null; revision: string | null; dirty: boolean };
export type Project = { id: string; name: string; path: string; lastOpenedAt: string; git: GitStatus };
