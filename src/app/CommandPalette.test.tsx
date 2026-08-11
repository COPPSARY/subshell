import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { expect, it, vi } from "vitest";
import { CommandPalette } from "./CommandPalette";

it("filters, runs, closes, and restores focus from the keyboard", async () => {
  const run = vi.fn();
  function Harness() {
    const [open, setOpen] = useState(false);
    return <><button onClick={() => setOpen(true)} type="button">Commands</button><CommandPalette commands={[{ id: "agents", label: "Open Agents", detail: "View active runs", run }]} onClose={() => setOpen(false)} open={open} /></>;
  }
  render(<Harness />);
  const trigger = screen.getByRole("button", { name: "Commands" });
  trigger.focus(); fireEvent.click(trigger);
  const input = screen.getByRole("combobox", { name: "Search commands" });
  await waitFor(() => expect(document.activeElement).toBe(input));
  fireEvent.change(input, { target: { value: "agents" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(run).toHaveBeenCalledOnce();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(trigger);
});
