import { describe, expect, it } from "vitest";
import type { GrpcMethod, GrpcSchema, GrpcStatus } from "../../ipc/bindings";
import { emptyContent } from "../request/content";
import {
  allowedActions,
  differsFromTemplate,
  findMethod,
  grpcSettingsOf,
  messageAfterPick,
  methodLabel,
  parseImportPaths,
  statusTone,
  transportOf,
  type CallStatus,
} from "./grpcModel";

const method = (over: Partial<GrpcMethod> = {}): GrpcMethod => ({
  name: "Say",
  kind: "unary",
  input_type: "a.In",
  output_type: "a.Out",
  template: '{\n  "name": ""\n}',
  ...over,
});
const schema: GrpcSchema = {
  source: "protos",
  services: [{ full_name: "a.Greeter", methods: [method(), method({ name: "Chat", kind: "bidi" })] }],
  files: [],
  warnings: [],
  fetched_at_ms: null,
};
const status = (code: number): GrpcStatus => ({ code, name: "X", message: "", details_base64: null });

describe("allowedActions", () => {
  it.each([
    ["unary", "idle", { start: true, send: false, end: false, cancel: false }],
    ["unary", "open", { start: false, send: false, end: false, cancel: true }],
    ["server_streaming", "open", { start: false, send: false, end: false, cancel: true }],
    ["client_streaming", "open", { start: false, send: true, end: true, cancel: true }],
    ["bidi", "open", { start: false, send: true, end: true, cancel: true }],
    ["bidi", "half_closed", { start: false, send: false, end: false, cancel: true }],
    ["bidi", "connecting", { start: false, send: false, end: false, cancel: true }],
    ["bidi", "closing", { start: false, send: false, end: false, cancel: false }],
    ["bidi", "finished", { start: true, send: false, end: false, cancel: false }],
    [null, "idle", { start: true, send: false, end: false, cancel: false }],
  ] as const)("%s while %s", (kind, callStatus: CallStatus, expected) => {
    expect(allowedActions(kind, callStatus)).toEqual(expected);
  });
});

describe("grpcModel helpers", () => {
  it("tones: OK, cancelled and failures differ", () => {
    expect(statusTone(status(0))).toBe("ok");
    expect(statusTone(status(1))).toBe("muted");
    expect(statusTone(status(14))).toBe("error");
  });

  it("labels and finds the selected method", () => {
    expect(methodLabel("a.Greeter", "Say")).toBe("a.Greeter/Say");
    expect(methodLabel("", "")).toBe("");
    expect(findMethod(schema, "a.Greeter", "Chat")?.kind).toBe("bidi");
    expect(findMethod(schema, "a.Greeter", "Nope")).toBeNull();
    expect(findMethod(null, "a.Greeter", "Say")).toBeNull();
  });

  it("compares the message with the template ignoring outer whitespace", () => {
    expect(differsFromTemplate(` ${method().template}\n`, method())).toBe(false);
    expect(differsFromTemplate("{}", method())).toBe(true);
    expect(differsFromTemplate("{}", null)).toBe(false);
  });

  it("fills the template on pick unless the user edited the message", () => {
    const say = method();
    const chat = method({ name: "Chat", template: '{ "text": "" }' });
    expect(messageAfterPick("", null, chat)).toBe(chat.template);
    expect(messageAfterPick(say.template, say, chat)).toBe(chat.template);
    expect(messageAfterPick('{"name":"mine"}', say, chat)).toBe('{"name":"mine"}');
  });

  it("detects the transport from the URL scheme", () => {
    expect(transportOf("https://h:443")).toBe("tls");
    expect(transportOf("HTTP://h:80")).toBe("plaintext");
    expect(transportOf("{{host}}")).toBe("unknown");
  });

  it("supplies an empty grpc section and parses import paths", () => {
    expect(grpcSettingsOf(emptyContent()).source).toBe("protos");
    expect(parseImportPaths(" ., third_party,, ")).toEqual([".", "third_party"]);
  });
});
