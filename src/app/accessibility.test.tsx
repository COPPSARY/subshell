import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import axe from "axe-core";
import { render } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { CommandPalette } from "./CommandPalette";
import { SafeQuitDialog } from "./SafeQuitDialog";

it("keeps command and quit dialogs free of automated semantic violations", async () => {
  const { container } = render(<><CommandPalette commands={[{ id: "projects", label: "Open Projects", detail: "Open a repository", run: vi.fn() }]} onClose={vi.fn()} open /><SafeQuitDialog activeRuns={2} onDecision={vi.fn()} /></>);
  const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
  expect(results.violations.map((violation) => violation.id)).toEqual([]);
});

it("keeps product text colors at WCAG AA contrast", () => {
  const css = readFileSync(resolve(process.cwd(), "src/styles.css"), "utf8");
  const color = (name: string) => css.match(new RegExp(`--color-${name}:\\s*(#[0-9a-f]{6})`, "i"))?.[1] ?? "";
  for (const [foreground, background] of [["primary", "app"], ["secondary", "app"], ["tertiary", "selected"], ["tertiary", "panel"]]) {
    expect(contrast(color(foreground), color(background)), `${foreground} on ${background}`).toBeGreaterThanOrEqual(4.5);
  }
});

function contrast(first: string, second: string) {
  const values = [luminance(first), luminance(second)].sort((left, right) => right - left);
  return (values[0] + 0.05) / (values[1] + 0.05);
}
function luminance(hex: string) {
  const channels = hex.slice(1).match(/../g)!.map((part) => parseInt(part, 16) / 255).map((value) => value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}
