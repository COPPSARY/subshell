import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getHealth } from "../features/health";
import { AppShell } from "./AppShell";

vi.mock("../features/health", () => ({ getHealth: vi.fn() }));

const mockedHealth = vi.mocked(getHealth);

describe("AppShell", () => {
  beforeEach(() => {
    mockedHealth.mockReset();
    mockedHealth.mockResolvedValue({ status: "ok", schemaVersion: 4 });
  });

  it("navigates all independently owned feature views", async () => {
    render(<AppShell />);
    expect(await screen.findByRole("heading", { name: "Open your first project" })).toBeTruthy();

    const destinations = [
      ["Projects", "Open your first project"],
      ["Activity", "No activity yet"],
      ["Tasks", "No tasks yet"],
      ["Agents", "Agents"],
      ["Providers", "AI agents"],
    ];

    for (const [button, heading] of destinations) {
      const destination = screen.getByRole("button", { name: button });
      fireEvent.click(destination);
      expect(screen.getByRole("heading", { name: heading })).toBeTruthy();
      expect(destination.getAttribute("aria-current")).toBe("page");
    }
  });

  it("keeps successful backend health internal", () => {
    mockedHealth.mockReturnValueOnce(new Promise(() => undefined));
    render(<AppShell />);

    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByText("Quick open")).toBeNull();
  });

  it("keeps navigation available when backend health fails", async () => {
    mockedHealth.mockRejectedValueOnce(new Error("unavailable"));
    render(<AppShell />);
    expect((await screen.findByRole("status")).textContent).toContain("Backend unavailable");
    expect(screen.getByRole("button", { name: "Tasks" })).toBeTruthy();
  });
});
