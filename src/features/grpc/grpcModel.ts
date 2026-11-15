import type {
  GrpcMethod,
  GrpcMethodKind,
  GrpcSchema,
  GrpcSettings,
  GrpcStatus,
  RequestContent,
} from "../../ipc/bindings";

export type CallStatus = "idle" | "connecting" | "open" | "half_closed" | "closing" | "finished";

export interface CallActions {
  readonly start: boolean;
  readonly send: boolean;
  readonly end: boolean;
  readonly cancel: boolean;
}

export const MAX_DEADLINE_MS = 3_600_000;
const CANCELLED_CODE = 1;
const OK_CODE = 0;

const NO_GRPC: GrpcSettings = {
  source: "protos",
  protos: [],
  import_paths: [],
  service: "",
  method: "",
  message: "",
  deadline_ms: 0,
};

/** The `grpc` section, which older content objects may lack. */
export const grpcSettingsOf = (content: RequestContent): GrpcSettings => content.grpc ?? NO_GRPC;

const streamsFromClient = (kind: GrpcMethodKind | null): boolean => kind === "client_streaming" || kind === "bidi";

/** Which buttons apply for a method kind in a call state. Send and End only on client-streaming kinds while open. */
export function allowedActions(kind: GrpcMethodKind | null, status: CallStatus): CallActions {
  const idle = status === "idle" || status === "finished";
  return {
    start: idle,
    send: status === "open" && streamsFromClient(kind),
    end: status === "open" && streamsFromClient(kind),
    cancel: status === "connecting" || status === "open" || status === "half_closed",
  };
}

export const STATUS_LABEL: Record<CallStatus, string> = {
  idle: "Idle",
  connecting: "Connecting",
  open: "Running",
  half_closed: "Waiting for response",
  closing: "Cancelling",
  finished: "Finished",
};

export type StatusTone = "ok" | "muted" | "error";

export function statusTone(status: GrpcStatus): StatusTone {
  if (status.code === OK_CODE) return "ok";
  return status.code === CANCELLED_CODE ? "muted" : "error";
}

export const KIND_TAG: Record<GrpcMethodKind, string> = {
  unary: "unary",
  server_streaming: "server",
  client_streaming: "client",
  bidi: "bidi",
};

export const methodLabel = (service: string, method: string): string => (service === "" ? "" : `${service}/${method}`);

export function findMethod(schema: GrpcSchema | null, service: string, method: string): GrpcMethod | null {
  const found = schema?.services.find((s) => s.full_name === service);
  return found?.methods.find((m) => m.name === method) ?? null;
}

/** Whether the editor text is something other than the method's template (whitespace aside). */
export function differsFromTemplate(message: string, method: GrpcMethod | null): boolean {
  return method !== null && message.trim() !== method.template.trim();
}

/** The message after picking `next`: the template replaces an empty message or the previous method's untouched template. */
export function messageAfterPick(current: string, previous: GrpcMethod | null, next: GrpcMethod): string {
  const untouched = current.trim() === "" || (previous !== null && !differsFromTemplate(current, previous));
  return untouched ? next.template : current;
}

/** `http://` is plaintext h2c; a `{{var}}` start cannot be told until resolved. */
export function transportOf(url: string): "tls" | "plaintext" | "unknown" {
  const lower = url.trim().toLowerCase();
  if (lower.startsWith("https://")) return "tls";
  return lower.startsWith("http://") ? "plaintext" : "unknown";
}

/** Splits comma-separated import paths, dropping empties. */
export const parseImportPaths = (text: string): string[] =>
  text.split(",").map((p) => p.trim()).filter((p) => p !== "");
