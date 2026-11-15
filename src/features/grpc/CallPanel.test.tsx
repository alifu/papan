import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { scratchLocation } from "../locations";
import * as api from "./api";
import { CallPanel } from "./CallPanel";
import { useGrpcStore, type GrpcTab } from "./grpcStore";

vi.mock("./api");

const loc = scratchLocation("s1");
const base: GrpcTab = {
  status: "finished",
  attempt: 1,
  minAttempt: 1,
  started: null,
  headers: null,
  log: [],
  grpcStatus: null,
  trailers: [],
  by: null,
  elapsedMs: null,
  error: null,
  restored: true,
  schema: null,
  schemaError: null,
  schemaLoading: false,
  schemaNeedsRefresh: false,
};
const set = (over: Partial<GrpcTab>) => useGrpcStore.setState({ byTab: { t: { ...base, ...over } } });

describe("CallPanel", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useGrpcStore.getState().reset();
  });

  it("shows the status line with the code name, message and elapsed time", () => {
    set({ grpcStatus: { code: 5, name: "NOT_FOUND", message: "no such user", details_base64: null }, elapsedMs: 12.4 });
    render(<CallPanel tabId="t" location={loc} />);
    expect(screen.getByLabelText("gRPC status")).toHaveTextContent("NOT_FOUND: no such user · 12 ms");
  });

  it("shows the message count and the messages", () => {
    set({ log: [{ seq: 1, at_ms: 1, direction: "received", frame: "text", text: '{"hello":"x"}', size: 13, truncated: false }] });
    render(<CallPanel tabId="t" location={loc} />);
    expect(screen.getByRole("tab", { name: /Messages/ })).toHaveTextContent("1");
    expect(screen.getByText(/hello/)).toBeInTheDocument();
  });

  it("shows headers and trailers", async () => {
    set({ headers: [{ name: "x-h", value: "1", enabled: true }], trailers: [{ name: "x-t", value: "2", enabled: true }] });
    render(<CallPanel tabId="t" location={loc} />);
    await userEvent.click(screen.getByRole("tab", { name: "Headers" }));
    expect(screen.getByText("x-h")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: "Trailers" }));
    expect(screen.getByText("x-t")).toBeInTheDocument();
  });

  it("says when there are no trailers", async () => {
    set({});
    render(<CallPanel tabId="t" location={loc} />);
    await userEvent.click(screen.getByRole("tab", { name: "Trailers" }));
    expect(screen.getByText("No trailers.")).toBeInTheDocument();
  });

  it("shows an error alert", () => {
    set({ error: { code: "network", message: "refused", detail: null } });
    render(<CallPanel tabId="t" location={loc} />);
    expect(screen.getByRole("alert")).toHaveTextContent("refused");
  });

  it("restores the log on mount", () => {
    vi.mocked(api.grpcLog).mockResolvedValue([]);
    useGrpcStore.setState({ byTab: {} });
    render(<CallPanel tabId="t" location={loc} />);
    expect(api.grpcLog).toHaveBeenCalled();
  });
});
