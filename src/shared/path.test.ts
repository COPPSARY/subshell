import { expect, it } from "vitest";
import { displayPath } from "./path";

it("hides Windows extended-length prefixes", () => {
  expect(displayPath("\\\\?\\C:\\Users\\test\\project")).toBe("C:\\Users\\test\\project");
  expect(displayPath("\\\\?\\UNC\\server\\share\\project")).toBe("\\\\server\\share\\project");
});

it("leaves ordinary paths unchanged", () => {
  expect(displayPath("C:\\Users\\test\\project")).toBe("C:\\Users\\test\\project");
  expect(displayPath("/tmp/project")).toBe("/tmp/project");
});
