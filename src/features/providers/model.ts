export type ProviderAccountStatus = "active" | "needs_reauth" | "revoked";
export type GenericProfile = { id: string; displayName: string; providerType?: string; status?: ProviderAccountStatus; executablePath: string; arguments: string[]; resumeArguments?: string[]; promptMode: "argument" | "stdin"; configRootEnvVar: string | null; configSourcePath: string | null; inheritUserHome: boolean };
export type DetectedProvider = { key: string; displayName: string; executablePath: string; arguments: string[]; resumeArguments: string[]; promptMode: "argument" | "stdin"; isConfigured: boolean };
