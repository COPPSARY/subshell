import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getHealth } from "../features/health";
import { AppShell } from "./AppShell";

vi.mock("../features/health", () => ({ getHealth: vi.fn() }));

const mockedHealth = vi.mocked(getHealth);

describe("AppShell", () => {
  beforeEach(() => {
    mockedHealth.mockReset();
    mockedHealth.mockResolvedValue({ status: "ok", schemaVersion: 2 });
  });

  it("navigates all independently owned feature views", async () => {
    render(<AppShell />);
    expect(await screen.findByText("Schema 2 · Ready")).toBeTruthy();

    const destinations = [
      ["Projects", "No repositories yet"],
      ["Timeline", "No activity yet"],
      ["Tasks", "No tasks yet"],
      ["Providers", "Generic CLI profiles"],
    ];

    for (const [button, heading] of destinations) {
      const destination = screen.getByRole("button", { name: button });
      fireEvent.click(destination);
      expect(screen.getByRole("heading", { name: heading })).toBeTruthy();
      expect(destination.getAttribute("aria-current")).toBe("page");
    }
  });

  it("shows an accessible connection status without advertising unfinished controls", () => {
    mockedHealth.mockReturnValueOnce(new Promise(() => undefined));
    render(<AppShell />);

    expect(screen.getByRole("status").textContent).toContain("Connecting to backend");
    expect(screen.queryByText("Quick open")).toBeNull();
  });

  it("keeps navigation available when backend health fails", async () => {
    mockedHealth.mockRejectedValueOnce(new Error("unavailable"));
    render(<AppShell />);
    expect((await screen.findByRole("status")).textContent).toContain("Backend unavailable");
    expect(screen.getByRole("button", { name: "Tasks" })).toBeTruthy();
  });
});
