import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { WsMessage } from "../../ipc/bindings";
import { MessageLog } from "./MessageLog";

const msg = (seq: number, over: Partial<WsMessage> = {}): WsMessage => ({
  seq,
  at_ms: new Date(2026, 0, 1, 10, 11, 12, 13).getTime(),
  direction: "received",
  frame: "text",
  text: `m${seq}`,
  size: 2,
  truncated: false,
  ...over,
});
const rows = () => screen.getAllByRole("listitem");

describe("MessageLog", () => {
  it("shows direction, time, frame, size and a preview per row", () => {
    render(<MessageLog messages={[msg(1, { direction: "sent", text: '{"a":1}', size: 7 })]} onClear={vi.fn()} />);
    const row = rows()[0]!;
    expect(within(row).getByLabelText("Sent")).toBeVisible();
    expect(row).toHaveTextContent("10:11:12.013");
    expect(row).toHaveTextContent("text");
    expect(row).toHaveTextContent("7 B");
    expect(row).toHaveTextContent('{"a":1}');
  });

  it("expands a row to pretty JSON and collapses it again", async () => {
    render(<MessageLog messages={[msg(1, { text: '{"a":1}' })]} onClear={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: /Expand message 1/ }));
    expect(screen.getByLabelText("Message 1 body")).toHaveTextContent('"a": 1');
    await userEvent.click(screen.getByRole("button", { name: /Collapse message 1/ }));
    expect(screen.queryByLabelText("Message 1 body")).toBeNull();
  });

  it("filters by direction and by text", async () => {
    render(
      <MessageLog
        messages={[msg(1, { direction: "sent", text: "alpha" }), msg(2, { text: "beta" }), msg(3, { text: "alphabet" })]}
        onClear={vi.fn()}
      />,
    );
    await userEvent.selectOptions(screen.getByLabelText("Direction"), "received");
    expect(rows()).toHaveLength(2);
    await userEvent.type(screen.getByLabelText("Filter text"), "alph");
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toHaveTextContent("alphabet");
  });

  it("says so when the filter hides everything and when there is nothing yet", async () => {
    const { rerender } = render(<MessageLog messages={[]} onClear={vi.fn()} />);
    expect(screen.getByText("No messages yet.")).toBeVisible();
    rerender(<MessageLog messages={[msg(1)]} onClear={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("Filter text"), "zzz");
    expect(screen.getByText("No messages match the filter.")).toBeVisible();
  });

  it("marks truncated frames and clears", async () => {
    const onClear = vi.fn();
    render(<MessageLog messages={[msg(1, { truncated: true })]} onClear={onClear} />);
    expect(rows()[0]).toHaveTextContent("truncated");
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(onClear).toHaveBeenCalled();
  });
});
