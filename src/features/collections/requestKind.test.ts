import { describe, expect, it } from "vitest";
import { kindBadge, kindIcon } from "./requestKind";

describe("requestKind", () => {
  it.each([
    ["http", "request-http", "GET", "GET"],
    ["graphql", "request-graphql", "POST", "GQL"],
    ["soap", "request-soap", "POST", "SOAP"],
    ["ws", "request-ws", "GET", "WS"],
    ["grpc", "request-grpc", "POST", "gRPC"],
    [null, "request-http", "PUT", "PUT"],
  ] as const)("maps %s to icon and badge", (kind, icon, method, badge) => {
    expect(kindIcon(kind)).toBe(icon);
    expect(kindBadge(kind, method)).toBe(badge);
  });

  it("keeps a null method for folders", () => {
    expect(kindBadge(null, null)).toBeNull();
  });
});
