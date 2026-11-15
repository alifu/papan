import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as collections from "../collections/api";
import * as graphqlApi from "../graphql/api";
import * as grpcApi from "../grpc/api";
import { useGrpcStore } from "../grpc/grpcStore";
import * as wsApi from "../ws/api";
import { useWsStore } from "../ws/wsStore";
import { scratchLocation } from "../locations";
import * as api from "./api";
import { emptyContent } from "./content";
import { RequestEditor } from "./RequestEditor";
import { useRequestStore } from "./requestStore";

vi.mock("../collections/api");
vi.mock("./api");
vi.mock("../graphql/api");
vi.mock("../ws/api");
vi.mock("../grpc/api");
vi.mock("../environments/api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));
vi.mock("../response/ResponseViewer", () => ({ ResponseViewer: () => <div>viewer</div> }));
vi.mock("../soap/SoapPanel", () => ({
  default: ({ onChange }: { onChange: (u: (c: { url: string }) => { url: string }) => void }) => (
    <button onClick={() => onChange((c) => ({ ...c, url: "http://soap" }))}>soap-panel</button>
  ),
}));

const loc = scratchLocation("s1");
const gql = (query: string, operation_name = "") => ({
  ...emptyContent(),
  kind: "graphql" as const,
  url: "http://x/graphql",
  method: "POST",
  body: { type: "graphql" as const, query, variables: "", operation_name },
});
const open = async (content: ReturnType<typeof emptyContent>) => {
  vi.mocked(collections.readRequest).mockResolvedValue({ name: "R", content });
  render(<RequestEditor tabId="t" location={loc} />);
  await screen.findByLabelText("URL");
};
const tabNames = () => screen.getAllByRole("tab").map((t) => t.textContent);

describe("RequestEditor by kind", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useRequestStore.setState({ byTab: {} });
    vi.mocked(graphqlApi.readCachedSchema).mockResolvedValue(null);
  });

  it("shows Query and Variables, and no Body or method, for GraphQL", async () => {
    await open(gql("{ a }"));
    expect(tabNames().slice(0, 4)).toEqual(["Params", "Headers", "Query", "Variables"]);
    expect(screen.queryByRole("tab", { name: "Body" })).toBeNull();
    expect(screen.queryByLabelText("Method")).toBeNull();
    await userEvent.click(screen.getByRole("tab", { name: "Query" }));
    expect(screen.getByLabelText("GraphQL query")).toHaveValue("{ a }");
    await userEvent.click(screen.getByRole("tab", { name: "Variables" }));
    await userEvent.type(screen.getByLabelText("GraphQL variables"), "1");
    expect(useRequestStore.getState().byTab.t?.draft?.body).toMatchObject({ type: "graphql", variables: "1" });
  });

  it("shows Start and the Subscription section, not Send, for a subscription, and never sends it", async () => {
    vi.mocked(wsApi.wsLog).mockResolvedValue([]);
    await open(gql("subscription S { s }"));
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    expect(screen.getByLabelText("Subscription endpoint")).toHaveTextContent("ws://x/graphql");
    expect(screen.getByText("No results yet.")).toBeVisible();
    await useRequestStore.getState().send("t", loc);
    expect(api.sendRequestOutcome).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("tab", { name: "Subscription" }));
    await userEvent.type(screen.getByLabelText("WebSocket URL"), "w");
    expect(useRequestStore.getState().byTab.t?.draft?.subscription).toMatchObject({ url: "w" });
  });

  it("enables Send once a query operation is selected", async () => {
    await open(gql("query A { a } subscription S { s }", "A"));
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("shows Body (XML only) and SOAP for SOAP and applies the panel's update", async () => {
    await open({ ...emptyContent(), kind: "soap", method: "POST", body: { type: "xml", text: "<a/>" } });
    expect(tabNames().slice(0, 4)).toEqual(["Params", "Headers", "Body", "SOAP"]);
    await userEvent.click(screen.getByRole("tab", { name: "Body" }));
    expect(within(screen.getByLabelText("Body type")).getAllByRole("option").map((o) => o.textContent)).toEqual(["XML"]);
    await userEvent.click(screen.getByRole("tab", { name: "SOAP" }));
    await userEvent.click(screen.getByRole("button", { name: "soap-panel" }));
    expect(useRequestStore.getState().byTab.t?.draft?.url).toBe("http://soap");
  });

  describe("gRPC", () => {
    const grpc = (over: Partial<ReturnType<typeof emptyContent>> = {}) => ({
      ...emptyContent(),
      kind: "grpc" as const,
      method: "POST",
      url: "https://h:443",
      grpc: { source: "protos" as const, protos: ["a.proto"], import_paths: [], service: "a.G", method: "Say", message: "", deadline_ms: 0 },
      ...over,
    });
    const schema = {
      source: "protos" as const,
      services: [{ full_name: "a.G", methods: [{ name: "Say", kind: "unary" as const, input_type: "a.In", output_type: "a.Out", template: '{"name":""}' }] }],
      files: [],
      warnings: [],
      fetched_at_ms: null,
    };
    beforeEach(() => {
      useGrpcStore.getState().reset();
      vi.mocked(grpcApi.grpcLog).mockResolvedValue([]);
      vi.mocked(grpcApi.grpcSchema).mockResolvedValue(schema);
    });

    it("shows the gRPC sections, Start instead of Send and no method verb", async () => {
      await open(grpc());
      expect(tabNames().slice(0, 5)).toEqual(["Message", "Metadata", "Protos", "Auth", "Settings"]);
      expect(await screen.findByRole("button", { name: "Start" })).toBeVisible();
      expect(screen.queryByLabelText("Method verb")).toBeNull();
      expect(screen.getByRole("textbox", { name: "Message" })).toBeVisible();
    });

    it("fills the template through the picker into the draft and starts with it", async () => {
      vi.mocked(grpcApi.grpcStart).mockResolvedValue({ call_id: "c", attempt: 1, path: "/a.G/Say", kind: "unary", url: "https://h:443" });
      vi.mocked(grpcApi.listenGrpcEvents).mockResolvedValue(() => {});
      await open(grpc());
      await screen.findByRole("option", { name: "Say (unary)" });
      await userEvent.selectOptions(screen.getByLabelText("Method"), "a.G/Say");
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue('{"name":""}');
      await userEvent.click(screen.getByRole("button", { name: "Start" }));
      expect(grpcApi.grpcStart).toHaveBeenCalledWith(
        expect.stringMatching(/^[0-9a-f-]{36}$/),
        loc,
        expect.objectContaining({ kind: "grpc", grpc: expect.objectContaining({ message: '{"name":""}' }) }),
      );
    });

    it("edits the deadline into the draft and hides redirect settings", async () => {
      await open(grpc());
      await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
      expect(screen.queryByLabelText("HTTP version")).toBeNull();
      await userEvent.clear(screen.getByLabelText("Deadline"));
      await userEvent.type(screen.getByLabelText("Deadline"), "5000");
      expect(useRequestStore.getState().byTab.t?.draft?.grpc?.deadline_ms).toBe(5000);
    });

    it("rejects a bad deadline", async () => {
      await open(grpc());
      await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
      await userEvent.type(screen.getByLabelText("Deadline"), "x");
      expect(screen.getByRole("alert")).toHaveTextContent("Deadline must be");
    });

    it("shows the -bin hint and edits metadata into the draft headers", async () => {
      await open(grpc());
      await userEvent.click(screen.getByRole("tab", { name: "Metadata" }));
      expect(screen.getByText(/-bin/)).toBeVisible();
      await userEvent.type(screen.getByLabelText("Name 1"), "x");
      expect(useRequestStore.getState().byTab.t?.draft?.headers[0]?.name).toBe("x");
    });
  });

  describe("WebSocket", () => {
    const ws = (over: Partial<ReturnType<typeof emptyContent>> = {}) => ({
      ...emptyContent(),
      kind: "ws" as const,
      url: "wss://x/ws",
      ws: { subprotocols: ["chat"], messages: [{ name: "Join", format: "json" as const, text: '{"t":"join"}' }] },
      ...over,
    });
    beforeEach(() => {
      useWsStore.getState().reset();
      vi.mocked(wsApi.wsLog).mockResolvedValue([]);
    });

    it("shows the WebSocket sections, Connect instead of Send and no method", async () => {
      await open(ws());
      expect(tabNames()).toEqual(["Params", "Headers", "Messages", "Subprotocols", "Auth", "Settings"]);
      expect(screen.getByRole("button", { name: "Connect" })).toBeVisible();
      expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
      expect(screen.queryByLabelText("Method")).toBeNull();
      expect(screen.getByLabelText("Message")).toBeVisible();
    });

    it("connects with the current draft", async () => {
      vi.mocked(wsApi.wsConnect).mockResolvedValue({ connection_id: "t", url: "wss://x/ws", protocol: null, status: 101, headers: [] });
      vi.mocked(wsApi.listenWsEvents).mockResolvedValue(() => {});
      await open(ws());
      await userEvent.click(screen.getByRole("button", { name: "Connect" }));
      expect(wsApi.wsConnect).toHaveBeenCalledWith(expect.stringMatching(/^[0-9a-f-]{36}$/), loc, expect.objectContaining({ kind: "ws", url: "wss://x/ws" }));
      expect(await screen.findByRole("button", { name: "Disconnect" })).toBeVisible();
    });

    it("loads a template into the composer", async () => {
      await open(ws());
      await userEvent.click(screen.getByRole("tab", { name: "Messages" }));
      await userEvent.click(screen.getByRole("button", { name: "Load Join into composer" }));
      expect(screen.getByLabelText("Message")).toHaveValue('{"t":"join"}');
      expect(screen.getByLabelText("Format")).toHaveValue("json");
    });

    it("edits subprotocols into the draft", async () => {
      await open(ws());
      await userEvent.click(screen.getByRole("tab", { name: "Subprotocols" }));
      await userEvent.type(screen.getByLabelText("New subprotocol"), "chat.v2{Enter}");
      expect(useRequestStore.getState().byTab.t?.draft?.ws?.subprotocols).toEqual(["chat", "chat.v2"]);
    });

    it("hides redirect and HTTP version settings", async () => {
      await open(ws());
      await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
      expect(screen.queryByLabelText("HTTP version")).toBeNull();
      expect(screen.getByLabelText("Verify TLS certificates")).toBeVisible();
    });
  });
});
