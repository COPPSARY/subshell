import { expect, it } from "vitest";
import { errorMessage } from "./error";

it("reads structured backend errors without leaking object coercion", () => {
  expect(errorMessage({ code: "process_error", message: "Codex could not be launched", retryable: false })).toBe("Codex could not be launched");
  expect(errorMessage({ code: "process_error" }, "Launch failed")).toBe("Launch failed");
  expect(errorMessage({ code: "process_error" })).not.toBe("[object Object]");
});
