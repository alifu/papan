import type {
  Callback,
  DataBucket,
  KeyValue,
  MockEnvironment,
  MockLogEntry,
  MockResponse,
  MockRoute,
  ResponseMode,
  Rule,
  RuleOperator,
  RuleTarget,
} from "../../ipc/bindings";

// The generated types make serde-defaulted fields optional; the editors work on these,
// where every field is present (`normalizeMock` fills them in after a read).
export type MRule = Required<Rule>;
export type MResponse = Required<
  Omit<MockResponse, "headers" | "rules" | "callbacks">
> & {
  headers: KeyValue[];
  rules: MRule[];
  callbacks: Callback[];
};
export type MRoute = Required<Omit<MockRoute, "responses">> & {
  responses: MResponse[];
};
export type MEnv = Required<
  Omit<MockEnvironment, "headers" | "routes" | "data" | "fallback">
> & {
  data: DataBucket[];
  fallback: MResponse | null;
  headers: KeyValue[];
  routes: MRoute[];
};

const normalizeRule = (rule: Rule): MRule => ({
  target: rule.target,
  modifier: rule.modifier ?? "",
  operator: rule.operator,
  value: rule.value ?? "",
  invert: rule.invert ?? false,
});

const normalizeResponse = (response: MockResponse): MResponse => ({
  id: response.id,
  label: response.label ?? "",
  status: response.status,
  headers: [...(response.headers ?? [])],
  body: response.body ?? "",
  latency_ms: response.latency_ms ?? 0,
  rules: (response.rules ?? []).map(normalizeRule),
  rules_operator: response.rules_operator ?? "and",
  default: response.default ?? false,
  file: response.file ?? "",
  callbacks: [...(response.callbacks ?? [])],
});

/** Fills in every field the file left to its default. */
export const normalizeMock = (mock: MockEnvironment): MEnv => ({
  name: mock.name,
  port: mock.port ?? 0,
  bind: mock.bind ?? "loopback",
  prefix: mock.prefix ?? "",
  latency_ms: mock.latency_ms ?? 0,
  cors: mock.cors ?? true,
  tls: mock.tls ?? false,
  proxy: mock.proxy ?? "",
  data: [...(mock.data ?? [])],
  fallback: mock.fallback ? normalizeResponse(mock.fallback) : null,
  headers: [...(mock.headers ?? [])],
  routes: (mock.routes ?? []).map((route) => ({
    id: route.id,
    method: route.method,
    endpoint: route.endpoint,
    enabled: route.enabled ?? true,
    mode: route.mode ?? "rules",
    kind: route.kind ?? "http",
    bucket: route.bucket ?? "",
    id_key: route.id_key ?? "id",
    responses: (route.responses ?? []).map(normalizeResponse),
  })),
});

export const METHODS = [
  "get",
  "post",
  "put",
  "patch",
  "delete",
  "head",
  "options",
  "any",
] as const;

export const MODES: readonly {
  readonly value: ResponseMode;
  readonly label: string;
}[] = [
  { value: "rules", label: "First matching rules" },
  { value: "sequential", label: "Sequential" },
  { value: "random", label: "Random" },
  { value: "disable_rules", label: "Always the default" },
];

export const TARGETS: readonly {
  readonly value: RuleTarget;
  readonly label: string;
}[] = [
  { value: "body", label: "Body" },
  { value: "query", label: "Query param" },
  { value: "header", label: "Header" },
  { value: "params", label: "Route param" },
  { value: "cookie", label: "Cookie" },
  { value: "request_number", label: "Request number" },
  { value: "path", label: "Path" },
];

export const OPERATORS: readonly {
  readonly value: RuleOperator;
  readonly label: string;
}[] = [
  { value: "equals", label: "equals" },
  { value: "regex", label: "matches regex" },
  { value: "regex_i", label: "matches regex (ignore case)" },
  { value: "null", label: "is absent" },
  { value: "empty_array", label: "is an empty array" },
  { value: "array_includes", label: "array includes" },
];

/** Targets whose rule needs a name or path next to it. */
export const NEEDS_MODIFIER: ReadonlySet<RuleTarget> = new Set([
  "body",
  "query",
  "header",
  "params",
  "cookie",
]);
export const NO_VALUE_OPERATORS: ReadonlySet<RuleOperator> = new Set([
  "null",
  "empty_array",
]);

export const MAX_PORT = 65_535;
export const MAX_LATENCY_MS = 60_000;

export function newId(): string {
  const crypto = globalThis.crypto as Crypto | undefined;
  return (
    crypto?.randomUUID?.() ??
    `id-${Math.random().toString(36).slice(2)}-${Date.now()}`
  );
}

export const newResponse = (isDefault: boolean): MResponse => ({
  id: newId(),
  label: "",
  status: 200,
  headers: [],
  body: "",
  latency_ms: 0,
  rules: [],
  rules_operator: "and",
  default: isDefault,
  file: "",
  callbacks: [],
});

export const newRoute = (): MRoute => ({
  id: newId(),
  method: "get",
  endpoint: "",
  enabled: true,
  mode: "rules",
  kind: "http",
  bucket: "",
  id_key: "id",
  responses: [newResponse(true)],
});

export const MAX_CALLBACKS = 5;
export const MAX_BUCKETS = 20;

export const newCallback = (): Callback => ({
  method: "post",
  url: "",
  headers: [],
  body: "",
  delay_ms: 0,
});

export const newBucket = (name: string): DataBucket => ({
  id: newId(),
  name,
  value: "[]",
});

/** A route that serves a data bucket as a REST resource. */
export const newDataRoute = (bucketId: string): MRoute => ({
  ...newRoute(),
  kind: "crud",
  bucket: bucketId,
  responses: [],
});

/** The answer for requests that match no route. */
export const newFallback = (): MResponse => ({
  ...newResponse(true),
  status: 404,
  body: '{"error": "Not found"}',
});

export const newRule = (): MRule => ({
  target: "query",
  modifier: "",
  operator: "equals",
  value: "",
  invert: false,
});

export const newMock = (name: string): MEnv => ({
  name,
  port: 0,
  bind: "loopback",
  prefix: "",
  latency_ms: 0,
  cors: true,
  tls: false,
  proxy: "",
  data: [],
  fallback: null,
  headers: [],
  routes: [],
});

export const replaceAt = <T>(
  items: readonly T[],
  index: number,
  next: T,
): T[] => items.map((item, i) => (i === index ? next : item));

export const removeAt = <T>(items: readonly T[], index: number): T[] =>
  items.filter((_, i) => i !== index);

/** Makes `index` the only default response of the route. */
export function withDefault(route: MRoute, index: number): MRoute {
  return {
    ...route,
    responses: route.responses.map((response, i) => ({
      ...response,
      default: i === index,
    })),
  };
}

/** Removes a response; the route keeps one, and a default if it had one. */
export function withoutResponse(route: MRoute, index: number): MRoute {
  if (route.responses.length <= 1) return route;
  const rest = removeAt(route.responses, index);
  const hasDefault = rest.some((r) => r.default);
  return {
    ...route,
    responses: hasDefault
      ? rest
      : replaceAt(rest, 0, { ...rest[0]!, default: true }),
  };
}

const isInsideCollection = (path: string): boolean =>
  path === "" ||
  !(
    /^[/\\]/.test(path) ||
    /[\0:]/.test(path) ||
    path.split(/[/\\]/).some((part) => part === ".." || part.startsWith("."))
  );

const HTTP_URL = /^https?:\/\/[^/\s@?#]+([/?#]|$)/i;

const parsesAsJson = (text: string): unknown => {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
};

function bucketsProblem(buckets: readonly DataBucket[]): string | null {
  if (buckets.length > MAX_BUCKETS) return "There are too many data buckets.";
  for (const [n, bucket] of buckets.entries()) {
    if (bucket.name.trim() === "") return `Data bucket ${n + 1} needs a name.`;
    if (buckets.slice(0, n).some((b) => b.name === bucket.name))
      return `Data bucket name “${bucket.name}” is used twice.`;
    if (parsesAsJson(bucket.value ?? "[]") === undefined)
      return `Data bucket “${bucket.name}” is not valid JSON.`;
  }
  return null;
}

function dataRouteProblem(
  at: string,
  route: MRoute,
  buckets: readonly DataBucket[],
): string | null {
  if (route.endpoint.replace(/^\/+|\/+$/g, "") === "")
    return `${at}: a data route needs an endpoint.`;
  if (route.id_key.trim() === "") return `${at}: give the id field a name.`;
  const bucket = buckets.find((b) => b.id === route.bucket);
  if (!bucket) return `${at}: choose a data bucket.`;
  if (!Array.isArray(parsesAsJson(bucket.value ?? "[]")))
    return `${at}: the data bucket “${bucket.name}” must hold a JSON array.`;
  return null;
}

function callbacksProblem(
  at: string,
  callbacks: readonly Callback[],
): string | null {
  if (callbacks.length > MAX_CALLBACKS) return `${at}: too many callbacks.`;
  for (const callback of callbacks) {
    if (!HTTP_URL.test(callback.url))
      return `${at}: a callback needs an http or https URL without credentials.`;
    if (
      (callback.delay_ms ?? 0) < 0 ||
      (callback.delay_ms ?? 0) > MAX_LATENCY_MS
    )
      return `${at}: a callback delay must be 0 to 60000 ms.`;
  }
  return null;
}

function responseProblem(at: string, response: MResponse): string | null {
  if (
    !Number.isInteger(response.status) ||
    response.status < 100 ||
    response.status > 599
  )
    return `${at}: the status must be 100 to 599.`;
  if (!isInsideCollection(response.file))
    return `${at}: the file must be a relative path inside the collection, with no hidden parts.`;
  return callbacksProblem(at, response.callbacks);
}

/** What blocks saving, or null. Mirrors the checks Rust makes. */
export function draftProblem(mock: MEnv): string | null {
  if (mock.name.trim() === "") return "Give the mock a name.";
  if (!Number.isInteger(mock.port) || mock.port < 0 || mock.port > MAX_PORT)
    return "The port must be 0 to 65535.";
  if (mock.latency_ms < 0 || mock.latency_ms > MAX_LATENCY_MS)
    return "Latency must be 0 to 60000 ms.";
  if (mock.proxy !== "" && !HTTP_URL.test(mock.proxy))
    return "The proxy target must be an http or https URL without credentials.";
  const bucketProblem = bucketsProblem(mock.data);
  if (bucketProblem) return bucketProblem;
  if (mock.fallback) {
    const problem = responseProblem("The fallback", mock.fallback);
    if (problem) return problem;
  }
  for (const [n, route] of mock.routes.entries()) {
    if (route.kind === "crud") {
      const problem = dataRouteProblem(`Route ${n + 1}`, route, mock.data);
      if (problem) return problem;
    }
    for (const response of route.responses) {
      const problem = callbacksProblem(`Route ${n + 1}`, response.callbacks);
      if (problem) return problem;
      if (!isInsideCollection(response.file)) {
        return `Route ${n + 1}: the file must be a relative path inside the collection, with no hidden parts.`;
      }
      if (
        !Number.isInteger(response.status) ||
        response.status < 100 ||
        response.status > 599
      ) {
        return `Route ${n + 1}: the status must be 100 to 599.`;
      }
      if (response.latency_ms < 0 || response.latency_ms > MAX_LATENCY_MS) {
        return `Route ${n + 1}: latency must be 0 to 60000 ms.`;
      }
    }
  }
  return null;
}

export const endpointLabel = (route: MRoute): string =>
  `${route.kind === "crud" ? "DATA" : route.method.toUpperCase()} /${route.endpoint.replace(/^\/+/, "")}`;

const slashes = /^\/+|\/+$/g;

/** A route that answers the request in a log entry (relative to the mock's prefix). */
export const routeFromRequest = (
  entry: Pick<MockLogEntry, "method" | "path">,
  prefix: string,
): MRoute => {
  const method = entry.method.toLowerCase();
  const wanted = prefix.replace(slashes, "");
  const path = entry.path.replace(slashes, "");
  const relative =
    wanted !== "" && (path === wanted || path.startsWith(`${wanted}/`))
      ? path.slice(wanted.length).replace(slashes, "")
      : path;
  return {
    ...newRoute(),
    method: (METHODS as readonly string[]).includes(method) ? method : "any",
    endpoint: relative,
  };
};
