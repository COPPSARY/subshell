import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getHealth } from "../features/health";
import { AppShell } from "./AppShell";

vi.mock("../features/health", () => ({ getHealth: vi.fn() }));
const eventMocks = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => eventMocks);

const mockedHealth = vi.mocked(getHealth);

describe("AppShell", () => {
  beforeEach(() => {
    mockedHealth.mockReset();
    mockedHealth.mockResolvedValue({ status: "ok", schemaVersion: 4 });
    eventMocks.listen.mockReset();
    eventMocks.listen.mockResolvedValue(() => undefined);
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

  it("asks for a safe decision instead of closing over active runs", async () => {
    render(<AppShell />);
    await waitFor(() => expect(eventMocks.listen).toHaveBeenCalled());
    act(() => eventMocks.listen.mock.calls[0][1]({ payload: 2 }));
    expect(screen.getByRole("dialog", { name: "Agents are still running" }).textContent).toContain("2 active runs");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
