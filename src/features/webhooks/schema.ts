import { z } from "zod";
import type { WebhookBind, WebhookConfig } from "../../ipc/bindings";

export const MAX_LISTENERS = 20;
export const MAX_RESPONSE_HEADERS = 50;
export const MAX_RESPONSE_BODY_BYTES = 1024 * 1024;
export const MIN_BODY_CAP = 1024;
export const MAX_BODY_CAP = 10 * 1024 * 1024;
export const MAX_CAPTURES_LIMIT = 1000;

const HEADER_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const FORBIDDEN_HEADERS = new Set(["content-length", "transfer-encoding"]);
const byteLength = (text: string): number => new TextEncoder().encode(text).length;

const headerRow = z
  .object({ name: z.string().trim(), value: z.string() })
  .superRefine((row, ctx) => {
    if (!HEADER_NAME.test(row.name)) {
      ctx.addIssue({ code: "custom", message: `"${row.name}" is not a valid header name.`, path: ["name"] });
    } else if (FORBIDDEN_HEADERS.has(row.name.toLowerCase())) {
      ctx.addIssue({ code: "custom", message: `${row.name} is set by the server.`, path: ["name"] });
    }
    if (/[\r\n\0]/.test(row.value)) {
      ctx.addIssue({ code: "custom", message: "Header values cannot contain line breaks.", path: ["value"] });
    }
  });

const pathRule = z
  .string()
  .min(1, "Path is required.")
  .max(256, "Path must be 256 characters or fewer.")
  .refine((p) => p.startsWith("/"), "Path must start with /.")
  .refine((p) => !p.includes(".."), "Path cannot contain ..")
  .refine((p) => !/[?#\s]/.test(p), "Path cannot contain ?, # or spaces.");

export const listenerFormSchema = z.object({
  name: z.string().trim().min(1, "Name is required.").max(64, "Name must be 64 characters or fewer."),
  bind: z.enum(["loopback", "all_interfaces"]),
  port: z
    .number("Port must be a number.")
    .int("Port must be a whole number.")
    .refine((p) => p === 0 || (p >= 1024 && p <= 65535), "Port must be 0 (any free port) or 1024-65535."),
  path: pathRule,
  status: z
    .number("Status must be a number.")
    .int()
    .min(200, "Status must be 200-599.")
    .max(599, "Status must be 200-599."),
  headers: z.array(headerRow).max(MAX_RESPONSE_HEADERS, `At most ${MAX_RESPONSE_HEADERS} response headers.`),
  body: z.string().refine((b) => byteLength(b) <= MAX_RESPONSE_BODY_BYTES, "Response body must be 1 MiB or smaller."),
  maxBodyBytes: z
    .number("Body limit must be a number.")
    .int()
    .min(MIN_BODY_CAP, "Body limit must be at least 1 KiB.")
    .max(MAX_BODY_CAP, "Body limit must be 10 MiB or smaller."),
  maxCaptures: z
    .number("Capture limit must be a number.")
    .int()
    .min(1, "Keep at least 1 capture.")
    .max(MAX_CAPTURES_LIMIT, `Keep at most ${MAX_CAPTURES_LIMIT} captures.`),
});

/** Editable form values; numbers are kept as text until validation. */
export interface ListenerForm {
  readonly name: string;
  readonly bind: WebhookBind;
  readonly port: string;
  readonly path: string;
  readonly status: string;
  readonly headers: readonly { readonly name: string; readonly value: string }[];
  readonly body: string;
  readonly maxBodyBytes: string;
  readonly maxCaptures: string;
}

export type ValidationResult =
  | { readonly ok: true; readonly config: WebhookConfig }
  | { readonly ok: false; readonly errors: readonly string[] };

const toNumber = (text: string): number => (text.trim() === "" ? Number.NaN : Number(text));

export function formFromConfig(config: WebhookConfig): ListenerForm {
  return {
    name: config.name,
    bind: config.bind,
    port: String(config.port),
    path: config.path,
    status: String(config.response.status),
    headers: config.response.headers.map(({ name, value }) => ({ name, value })),
    body: config.response.body,
    maxBodyBytes: String(config.max_body_bytes),
    maxCaptures: String(config.max_captures),
  };
}

export function validateForm(id: string, form: ListenerForm): ValidationResult {
  const parsed = listenerFormSchema.safeParse({
    ...form,
    port: toNumber(form.port),
    status: toNumber(form.status),
    maxBodyBytes: toNumber(form.maxBodyBytes),
    maxCaptures: toNumber(form.maxCaptures),
  });
  if (!parsed.success) {
    return { ok: false, errors: [...new Set(parsed.error.issues.map((issue) => issue.message))] };
  }
  const v = parsed.data;
  return {
    ok: true,
    config: {
      id,
      name: v.name,
      bind: v.bind,
      port: v.port,
      path: v.path,
      response: {
        status: v.status,
        headers: v.headers.map(({ name, value }) => ({ name, value, enabled: true })),
        body: v.body,
      },
      max_body_bytes: v.maxBodyBytes,
      max_captures: v.maxCaptures,
    },
  };
}

export const DEFAULT_CONFIG: WebhookConfig = {
  id: "",
  name: "Webhook",
  bind: "loopback",
  port: 0,
  path: "/hooks",
  response: {
    status: 200,
    headers: [{ name: "Content-Type", value: "text/plain; charset=utf-8", enabled: true }],
    body: "",
  },
  max_body_bytes: 1024 * 1024,
  max_captures: 200,
};
