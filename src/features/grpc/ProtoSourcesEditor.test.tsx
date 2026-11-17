import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RequestContent } from "../../ipc/bindings";
import { reportError } from "../../ui/toastStore";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { ProtoSourcesEditor } from "./ProtoSourcesEditor";
import { useGrpcStore, type GrpcTab } from "./grpcStore";

vi.mock("./api");
vi.mock("../../ui/toastStore", () => ({ reportError: vi.fn() }));

const loc = scratchLocation("s1");
const content = (over: Partial<NonNullable<RequestContent["grpc"]>> = {}): RequestContent => ({
  ...emptyContent(),
  kind: "grpc",
  grpc: { source: "protos", protos: ["a.proto"], import_paths: ["."], service: "", method: "", message: "", deadline_ms: 0, ...over },
});
const run = (onChange: ReturnType<typeof vi.fn>, base: RequestContent): RequestContent =>
  (onChange.mock.calls[onChange.mock.calls.length - 1]?.[0] as (c: RequestContent) => RequestContent)(base);
const reflect = vi.fn();
const tabWith = (over: Partial<GrpcTab>): GrpcTab => ({
  status: "idle",
  attempt: 0,
  minAttempt: 0,
  started: null,
  headers: null,
  log: [],
  grpcStatus: null,
  trailers: [],
  by: null,
  elapsedMs: null,
  error: null,
  restored: false,
  schema: null,
  schemaError: null,
  schemaLoading: false,
  schemaNeedsRefresh: false,
  ...over,
});

describe("ProtoSourcesEditor", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useGrpcStore.getState().reset();
    useGrpcStore.setState({ reflect });
  });

  it("imports a proto and appends what was added without duplicates", async () => {
    vi.mocked(api.grpcImportProto).mockResolvedValue({ added: ["a.proto", "b/c.proto"], warnings: [] });
    const onChange = vi.fn();
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Import .proto…" }));
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled());
    expect(run(onChange, content()).grpc?.protos).toEqual(["a.proto", "b/c.proto"]);
  });

  it("reports an import failure", async () => {
    vi.mocked(api.grpcImportProto).mockRejectedValue(new Error("x"));
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Import .proto…" }));
    await vi.waitFor(() => expect(reportError).toHaveBeenCalled());
  });

  it("removes a proto", async () => {
    const onChange = vi.fn();
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Remove a.proto" }));
    expect(run(onChange, content()).grpc?.protos).toEqual([]);
  });

  it("edits import paths", async () => {
    const onChange = vi.fn();
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("Import paths"), ", lib");
    expect(run(onChange, content()).grpc?.import_paths).toEqual([".", "lib"]);
  });

  it("shows import paths changed from outside", () => {
    const { rerender } = render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={vi.fn()} />);
    const reloaded = content({ import_paths: ["a", "b"] });
    rerender(<ProtoSourcesEditor tabId="t" location={loc} content={reloaded} onChange={vi.fn()} />);
    expect(screen.getByLabelText("Import paths")).toHaveValue("a, b");
  });

  it("switches the source", async () => {
    const onChange = vi.fn();
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={onChange} />);
    await userEvent.selectOptions(screen.getByLabelText("Source"), "reflection");
    expect(run(onChange, content()).grpc?.source).toBe("reflection");
  });

  it("shows compile errors and warnings", () => {
    useGrpcStore.setState({
      byTab: {
        t: tabWith({
          schema: { source: "protos", services: [], files: [], warnings: ["unused import"], fetched_at_ms: null },
          schemaError: { code: "invalid_input", message: "a.proto:3:1 syntax error", detail: null },
        }),
      },
    });
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content()} onChange={vi.fn()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("a.proto:3:1 syntax error");
    expect(screen.getByText("unused import")).toBeInTheDocument();
  });

  it("reflection: Refresh fetches and the cache time is shown", async () => {
    useGrpcStore.setState({
      byTab: { t: tabWith({ schema: { source: "reflection", services: [], files: [], warnings: [], fetched_at_ms: 1_700_000_000_000 } }) },
    });
    render(<ProtoSourcesEditor tabId="t" location={loc} content={content({ source: "reflection" })} onChange={vi.fn()} />);
    expect(screen.getByText(/Fetched/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Import .proto…" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(reflect).toHaveBeenCalledWith("t", loc, content({ source: "reflection" }));
  });

  it("reflection without a cache asks the user to load and warns that metadata is sent", async () => {
    useGrpcStore.setState({ byTab: { t: tabWith({ schemaNeedsRefresh: true }) } });
    const withUrl = { ...content({ source: "reflection" }), url: "https://api.test" };
    render(<ProtoSourcesEditor tabId="t" location={loc} content={withUrl} onChange={vi.fn()} />);
    expect(screen.getByText(/No cached schema/)).toBeInTheDocument();
    expect(screen.getByText(/Authorization/)).toHaveTextContent("https://api.test");
    await userEvent.click(screen.getByRole("button", { name: "Load schema from server" }));
    expect(reflect).toHaveBeenCalledWith("t", loc, withUrl);
  });
});
