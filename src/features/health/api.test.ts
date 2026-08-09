import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getHealth } from "./api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockedInvoke = vi.mocked(invoke);

describe("getHealth", () => {
  beforeEach(() => mockedInvoke.mockReset());

  it("uses the stable health command and returns its typed response", async () => {
    const response = { status: "ok" as const, schemaVersion: 1 };
    mockedInvoke.mockResolvedValueOnce(response);

    await expect(getHealth()).resolves.toEqual(response);
    expect(mockedInvoke).toHaveBeenCalledOnce();
    expect(mockedInvoke).toHaveBeenCalledWith("health_status");
  });

  it("preserves structured command failures", async () => {
    const error = {
      code: "storage_unavailable",
      message: "database unavailable",
      retryable: true,
    };
    mockedInvoke.mockRejectedValueOnce(error);

    await expect(getHealth()).rejects.toEqual(error);
  });
});
