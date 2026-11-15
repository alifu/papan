import type {
  KeyValue,
  MockEnvironment,
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
export type MResponse = Required<Omit<MockResponse, "headers" | "rules">> & {
  headers: KeyValue[];
  rules: MRule[];
};
export type MRoute = Required<Omit<MockRoute, "responses">> & {
  responses: MResponse[];
};
export type MEnv = Required<Omit<MockEnvironment, "headers" | "routes">> & {
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
});

/** Fills in every field the file left to its default. */
export const normalizeMock = (mock: MockEnvironment): MEnv => ({
  name: mock.name,
  port: mock.port ?? 0,
  bind: mock.bind ?? "loopback",
  prefix: mock.prefix ?? "",
  latency_ms: mock.latency_ms ?? 0,
  cors: mock.cors ?? true,
  headers: [...(mock.headers ?? [])],
  routes: (mock.routes ?? []).map((route) => ({
    id: route.id,
    method: route.method,
    endpoint: route.endpoint,
    enabled: route.enabled ?? true,
    mode: route.mode ?? "rules",
    responses: route.responses.map(normalizeResponse),
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
});

export const newRoute = (): MRoute => ({
  id: newId(),
  method: "get",
  endpoint: "",
  enabled: true,
  mode: "rules",
  responses: [newResponse(true)],
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

/** What blocks saving, or null. Mirrors the checks Rust makes. */
export function draftProblem(mock: MEnv): string | null {
  if (mock.name.trim() === "") return "Give the mock a name.";
  if (!Number.isInteger(mock.port) || mock.port < 0 || mock.port > MAX_PORT)
    return "The port must be 0 to 65535.";
  if (mock.latency_ms < 0 || mock.latency_ms > MAX_LATENCY_MS)
    return "Latency must be 0 to 60000 ms.";
  for (const [n, route] of mock.routes.entries()) {
    for (const response of route.responses) {
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
  `${route.method.toUpperCase()} /${route.endpoint.replace(/^\/+/, "")}`;
