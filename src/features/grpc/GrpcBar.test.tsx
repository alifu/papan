import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GrpcMethodKind, GrpcSchema, RequestContent } from "../../ipc/bindings";
import { scratchLocation } from "../locations";
import { emptyContent } from "../request/content";
import { GrpcBar } from "./GrpcBar";
import { SCHEMA_DEBOUNCE_MS } from "./useGrpcSchema";
import { useGrpcStore, type GrpcTab } from "./grpcStore";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));
vi.mock("../environments/usePreviewTitle", () => ({ usePreviewTitle: () => undefined }));
vi.mock("./api");

const loc = scratchLocation("s1");
const method = (name: string, kind: GrpcMethodKind) => ({ name, kind, input_type: "a.In", output_type: "a.Out", template: `{"${name}": 1}` });
const schema: GrpcSchema = {
  source: "protos",
  services: [
    { full_name: "a.Greeter", methods: [method("Say", "unary"), method("Chat", "bidi")] },
    { full_name: "a.Other", methods: [method("Watch", "server_streaming")] },
  ],
  files: [],
  warnings: [],
  fetched_at_ms: null,
};
const contentFor = (service: string, name: string, over: Partial<RequestContent> = {}): RequestContent => ({
  ...emptyContent(),
  kind: "grpc",
  method: "POST",
  url: "https://h:443",
  grpc: { source: "protos", protos: [], import_paths: [], service, method: name, message: "", deadline_ms: 0 },
  ...over,
});
const tab = (over: Partial<GrpcTab> = {}): GrpcTab => ({
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
  schema,
  schemaError: null,
  schemaLoading: false,
  schemaNeedsRefresh: false,
  ...over,
});
const fns = { start: vi.fn(), send: vi.fn(), end: vi.fn(), cancel: vi.fn(), loadSchema: vi.fn() };
const setup = (content: RequestContent, state: Partial<GrpcTab> = {}, onChange = vi.fn(), dirty = false) => {
  useGrpcStore.setState({ byTab: { t: tab(state) } });
  render(<GrpcBar tabId="t" location={loc} content={content} variables={[]} dirty={dirty} onUrl={vi.fn()} onChange={onChange} onSave={vi.fn()} />);
  return onChange;
};

describe("GrpcBar", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useGrpcStore.getState().reset();
    useGrpcStore.setState(fns);
  });

  it("groups methods by service with a kind tag", () => {
    setup(contentFor("a.Greeter", "Say"));
    const groups = screen.getAllByRole("group");
    expect(groups.map((g) => g.getAttribute("label"))).toEqual(["a.Greeter", "a.Other"]);
    expect(within(groups[1]!).getByRole("option")).toHaveTextContent("Watch (server)");
    expect(within(groups[0]!).getAllByRole("option").map((o) => o.textContent)).toEqual(["Say (unary)", "Chat (bidi)"]);
  });

  it("picking a method fills the template into an empty message", async () => {
    const onChange = setup(contentFor("", ""));
    await userEvent.selectOptions(screen.getByLabelText("Method"), "a.Other/Watch");
    const update = onChange.mock.calls[0]?.[0] as (c: RequestContent) => RequestContent;
    expect(update(contentFor("", "")).grpc).toMatchObject({ service: "a.Other", method: "Watch", message: '{"Watch": 1}' });
  });

  it("keeps an edited message when another method is picked", async () => {
    const onChange = setup(contentFor("a.Greeter", "Say"));
    await userEvent.selectOptions(screen.getByLabelText("Method"), "a.Greeter/Chat");
    const edited = contentFor("a.Greeter", "Say");
    const update = onChange.mock.calls[0]?.[0] as (c: RequestContent) => RequestContent;
    expect(update({ ...edited, grpc: { ...edited.grpc!, message: '{"mine":2}' } }).grpc?.message).toBe('{"mine":2}');
  });

  it("marks a method the schema does not contain", () => {
    setup(contentFor("a.Greeter", "Gone"));
    expect(screen.getByRole("option", { name: "a.Greeter/Gone (not found)" })).toBeInTheDocument();
  });

  it("shows the transport and starts with the current content", async () => {
    const content = contentFor("a.Greeter", "Say");
    setup(content);
    expect(screen.getByText("TLS")).toBeInTheDocument();
    expect(screen.getByLabelText("Call status")).toHaveTextContent("Idle");
    await userEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(fns.start).toHaveBeenCalledWith("t", loc, content);
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
  });

  it("shows Plaintext for http", () => {
    setup(contentFor("a.Greeter", "Say", { url: "http://h:80" }));
    expect(screen.getByText("Plaintext")).toBeInTheDocument();
  });

  it("disables Start until a method is picked", () => {
    setup(contentFor("", ""));
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
  });

  it("a unary call shows Cancel while running", async () => {
    setup(contentFor("a.Greeter", "Say"), { status: "open" });
    expect(screen.queryByRole("button", { name: "Start" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(fns.cancel).toHaveBeenCalledWith("t");
  });

  it("a bidi call shows Send and End while open", async () => {
    const content = contentFor("a.Greeter", "Chat");
    setup({ ...content, grpc: { ...content.grpc!, message: '{"x":1}' } }, { status: "open" });
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(fns.send).toHaveBeenCalledWith("t", loc, '{"x":1}');
    await userEvent.click(screen.getByRole("button", { name: "End" }));
    expect(fns.end).toHaveBeenCalledWith("t");
  });

  it("disables Send and End once half-closed", () => {
    setup(contentFor("a.Greeter", "Chat"), { status: "half_closed" });
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "End" })).toBeDisabled();
  });

  it("Mod-Enter starts, or sends while a stream is open", async () => {
    setup(contentFor("a.Greeter", "Chat"));
    await userEvent.click(screen.getByLabelText("URL"));
    await userEvent.keyboard("{Control>}{Enter}{/Control}");
    expect(fns.start).toHaveBeenCalledTimes(1);
  });

  it("Mod-Enter sends while open", async () => {
    setup(contentFor("a.Greeter", "Chat"), { status: "open" });
    await userEvent.click(screen.getByLabelText("URL"));
    await userEvent.keyboard("{Control>}{Enter}{/Control}");
    expect(fns.send).toHaveBeenCalledTimes(1);
    expect(fns.start).not.toHaveBeenCalled();
  });

  it("loads the schema once after the debounce", () => {
    vi.useFakeTimers();
    setup(contentFor("a.Greeter", "Say"));
    expect(fns.loadSchema).not.toHaveBeenCalled();
    vi.advanceTimersByTime(SCHEMA_DEBOUNCE_MS);
    expect(fns.loadSchema).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("enables Save only when dirty", () => {
    setup(contentFor("a.Greeter", "Say"), {}, vi.fn(), true);
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });
});
