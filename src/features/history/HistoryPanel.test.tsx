import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HistoryEntry } from "../../ipc/bindings";
import * as api from "./api";
import { HistoryPanel } from "./HistoryPanel";
import { useHistoryStore } from "./historyStore";
import { relativeTime } from "./relativeTime";

const open = vi.fn().mockReturnValue("t1");
vi.mock("../../shell/tabsStore", () => ({
  useTabsStore: (select: (s: { openRequest: typeof open }) => unknown) => select({ openRequest: open }),
}));
vi.mock("./api", () => ({ listHistory: vi.fn(), clearHistory: vi.fn(), restoreHistoryEntry: vi.fn() }));

const entry = (over: Partial<HistoryEntry>): HistoryEntry => ({
  id: "h1", sent_at_ms: Date.now() - 5 * 60_000, name: "n", method: "GET", url: "http://a.test/x",
  status: 200, error_code: null, total_ms: 3, size_bytes: 1, kind: "http", ...over,
});

beforeEach(() => {
  useHistoryStore.setState({ entries: [] });
  vi.mocked(api.listHistory).mockResolvedValue([entry({}), entry({ id: "h2", status: null, error_code: "timeout", sent_at_ms: null })]);
  vi.mocked(api.clearHistory).mockResolvedValue(null);
  vi.mocked(api.restoreHistoryEntry).mockResolvedValue({ id: "s1", name: "Restored", method: "GET", kind: "http" });
});

describe("HistoryPanel", () => {
  it("lists method, url, status or error and relative time", async () => {
    render(<HistoryPanel />);
    expect(await screen.findByText("200")).toBeInTheDocument();
    expect(screen.getByText("timeout")).toBeInTheDocument();
    expect(screen.getByText("5m ago")).toBeInTheDocument();
  });

  it("restores an entry and opens its tab on click", async () => {
    render(<HistoryPanel />);
    await userEvent.click((await screen.findAllByRole("button", { name: /GET/ }))[0]!);
    await waitFor(() => expect(open).toHaveBeenCalledWith({ scope: "scratch", id: "s1" }, "Restored"));
    expect(api.restoreHistoryEntry).toHaveBeenCalledWith("h1");
  });

  it("shows SOAP and GQL badges instead of POST", async () => {
    vi.mocked(api.listHistory).mockResolvedValue([
      entry({ id: "a", method: "POST", kind: "soap" }),
      entry({ id: "b", method: "POST", kind: "graphql" }),
    ]);
    render(<HistoryPanel />);
    expect(await screen.findByText("SOAP")).toBeInTheDocument();
    expect(screen.getByText("GQL")).toBeInTheDocument();
    expect(screen.queryByText("POST")).toBeNull();
  });

  it("clears the list", async () => {
    render(<HistoryPanel />);
    await screen.findByText("200");
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(await screen.findByText(/no requests sent yet/i)).toBeInTheDocument();
  });

  it("formats unknown and recent times", () => {
    expect(relativeTime(null, 1000)).toBe("");
    expect(relativeTime(1000, 2000)).toBe("just now");
    expect(relativeTime(0, 3 * 3600_000)).toBe("3h ago");
  });
});
