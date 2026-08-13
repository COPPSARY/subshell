import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { readRunOutput, readRunOutputTail, writeRunInput } from "./api";
import { RunTerminal, type SubscribeRunOutput } from "./RunTerminal";

const terminal = vi.hoisted(() => ({ focus: vi.fn(), input: null as null | ((data: string) => void), scrollToBottom: vi.fn(), write: vi.fn() }));
vi.mock("./api", () => ({ readRunOutput: vi.fn(), readRunOutputTail: vi.fn(), resizeRun: vi.fn().mockResolvedValue(undefined), writeRunInput: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@xterm/xterm", () => ({ Terminal: class { rows = 24; cols = 80; loadAddon() {} open() {} focus = terminal.focus; scrollToBottom = terminal.scrollToBottom; dispose() {} write(data: Uint8Array, callback?: () => void) { terminal.write(data); callback?.(); } onData(callback: (data: string) => void) { terminal.input = callback; return { dispose() {} }; } } }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));

beforeEach(() => { terminal.focus.mockReset(); terminal.input = null; terminal.scrollToBottom.mockReset(); terminal.write.mockReset(); vi.mocked(readRunOutput).mockImplementation(async (_runId, cursor) => ({ bytes: [], nextCursor: cursor ?? 0 })); vi.mocked(readRunOutputTail).mockReset(); vi.mocked(writeRunInput).mockClear(); });
const subscription = () => {
  let listener: Parameters<SubscribeRunOutput>[1] = () => undefined;
  return { emit: (chunk: Parameters<typeof listener>[0]) => listener(chunk), subscribe: vi.fn<SubscribeRunOutput>((_runId, next) => { listener = next; return () => undefined; }) };
};

it("restores terminal output when an existing session is opened", async () => {
  vi.mocked(readRunOutputTail).mockResolvedValue({ bytes: [65, 66], nextCursor: 2 });
  const output = subscription();
  render(<RunTerminal runId="run-1" subscribe={output.subscribe} />);
  await waitFor(() => expect(readRunOutputTail).toHaveBeenCalledWith("run-1"));
  await waitFor(() => expect(Array.from(terminal.write.mock.calls[0][0])).toEqual([65, 66]));
  expect(terminal.scrollToBottom).toHaveBeenCalled();
  expect(terminal.focus).not.toHaveBeenCalled();
  fireEvent.pointerDown(screen.getByLabelText("Interactive agent terminal"));
  expect(terminal.focus).toHaveBeenCalledOnce();
  terminal.input?.("cont"); terminal.input?.("inue");
  await waitFor(() => expect(writeRunInput).toHaveBeenCalledWith("run-1", Array.from(new TextEncoder().encode("continue"))));
  expect(writeRunInput).toHaveBeenCalledOnce();
});

it("keeps ended session logs read-only", async () => {
  vi.mocked(readRunOutputTail).mockResolvedValue({ bytes: [], nextCursor: 0 });
  const output = subscription();
  render(<RunTerminal interactive={false} runId="run-1" subscribe={output.subscribe} />);
  await waitFor(() => expect(readRunOutputTail).toHaveBeenCalledWith("run-1"));
  expect(screen.getByLabelText("Agent terminal log")).toBeTruthy();
  expect(terminal.input).toBeNull();
});

it("shows structured terminal connection errors", async () => {
  vi.mocked(readRunOutputTail).mockRejectedValue({ code: "output_unavailable", message: "Agent output is unavailable" });
  const output = subscription();

  render(<RunTerminal runId="run-1" subscribe={output.subscribe} />);

  expect((await screen.findByRole("alert")).textContent).toContain("Agent output is unavailable");
  expect(screen.queryByText("[object Object]")).toBeNull();
});

it("writes streamed output directly without a React rerender", async () => {
  vi.mocked(readRunOutputTail).mockResolvedValue({ bytes: [], nextCursor: 0 });
  const output = subscription();
  render(<RunTerminal runId="run-1" subscribe={output.subscribe} />);
  await waitFor(() => expect(output.subscribe).toHaveBeenCalledWith("run-1", expect.any(Function)));
  for (let cursor = 1; cursor <= 100; cursor += 1) output.emit({ bytes: [67], cursor });
  await waitFor(() => expect(terminal.write).toHaveBeenCalledOnce());
  expect(Array.from(terminal.write.mock.calls[0][0])).toHaveLength(100);
});
