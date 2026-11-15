import { describe, expect, it } from "vitest";
import {
  draftProblem,
  endpointLabel,
  newMock,
  newRoute,
  normalizeMock,
  removeAt,
  replaceAt,
  routeFromRequest,
  withDefault,
  withoutResponse,
} from "./mockModel";

describe("mockModel", () => {
  it("a new route has one default response", () => {
    const route = newRoute();
    expect(route.responses).toHaveLength(1);
    expect(route.responses[0]?.default).toBe(true);
  });

  it("replaceAt and removeAt do not mutate", () => {
    const list = [1, 2, 3] as const;
    expect(replaceAt(list, 1, 9)).toEqual([1, 9, 3]);
    expect(removeAt(list, 0)).toEqual([2, 3]);
    expect(list).toEqual([1, 2, 3]);
  });

  it("withDefault leaves exactly one default", () => {
    const route = {
      ...newRoute(),
      responses: [
        newRoute().responses[0]!,
        { ...newRoute().responses[0]!, default: false },
      ],
    };
    const next = withDefault(route, 1);
    expect(next.responses.map((r) => r.default)).toEqual([false, true]);
  });

  it("withoutResponse keeps one response and re-elects a default", () => {
    const one = newRoute();
    expect(withoutResponse(one, 0)).toBe(one);
    const two = {
      ...one,
      responses: [
        one.responses[0]!,
        { ...one.responses[0]!, id: "b", default: false },
      ],
    };
    const next = withoutResponse(two, 0);
    expect(next.responses).toHaveLength(1);
    expect(next.responses[0]?.default).toBe(true);
  });

  it("draftProblem names what is wrong", () => {
    const ok = newMock("Users");
    expect(draftProblem(ok)).toBeNull();
    expect(draftProblem({ ...ok, name: " " })).toMatch(/name/);
    expect(draftProblem({ ...ok, port: 70_000 })).toMatch(/port/);
    expect(draftProblem({ ...ok, latency_ms: 61_000 })).toMatch(/Latency/);
    const route = newRoute();
    const bad = {
      ...ok,
      routes: [
        { ...route, responses: [{ ...route.responses[0]!, status: 99 }] },
      ],
    };
    expect(draftProblem(bad)).toMatch(/status/);
  });

  it("labels an endpoint", () => {
    expect(
      endpointLabel({ ...newRoute(), method: "post", endpoint: "/users/:id" }),
    ).toBe("POST /users/:id");
  });
});

describe("11c fields", () => {
  it("defaults tls, proxy and file for old files", () => {
    const old = {
      name: "x",
      routes: [
        {
          id: "r",
          method: "get",
          endpoint: "a",
          responses: [{ id: "s", status: 200 }],
        },
      ],
    };
    const mock = normalizeMock(old as never);
    expect([mock.tls, mock.proxy, mock.routes[0]?.responses[0]?.file]).toEqual([
      false,
      "",
      "",
    ]);
  });

  it("refuses unsafe file paths and bad proxy targets", () => {
    for (const file of ["/etc/passwd", "../x", "a/.env", "C:x"]) {
      const mock = newMock("m");
      const route = newRoute();
      mock.routes = [
        { ...route, responses: [{ ...route.responses[0]!, file }] },
      ];
      expect(draftProblem(mock), file).toMatch(/file/);
    }
    for (const proxy of ["ftp://x", "api.example.com", "https://u:p@h"]) {
      expect(draftProblem({ ...newMock("m"), proxy }), proxy).toMatch(/proxy/);
    }
    expect(
      draftProblem({ ...newMock("m"), proxy: "https://api.example.com/v1" }),
    ).toBeNull();
  });

  it("builds a route from a logged request, relative to the prefix", () => {
    const route = routeFromRequest(
      { method: "POST", path: "/api/v1/users/7" },
      "/api/v1/",
    );
    expect([route.method, route.endpoint]).toEqual(["post", "users/7"]);
    expect(
      routeFromRequest({ method: "GET", path: "/other/x" }, "api").endpoint,
    ).toBe("other/x");
    expect(routeFromRequest({ method: "PROPFIND", path: "/" }, "").method).toBe(
      "any",
    );
  });
});

describe("11d fields", () => {
  it("keeps data buckets, the fallback, callbacks and data routes through a normalize", () => {
    const stored = {
      name: "x",
      data: [{ id: "b", name: "users", value: "[]" }],
      fallback: { id: "f", status: 418 },
      routes: [
        {
          id: "c",
          method: "get",
          endpoint: "users",
          kind: "crud",
          bucket: "b",
          id_key: "uid",
          responses: [],
        },
        {
          id: "h",
          method: "get",
          endpoint: "a",
          responses: [
            {
              id: "s",
              status: 200,
              callbacks: [{ method: "post", url: "https://x.test", body: "" }],
            },
          ],
        },
      ],
    };
    const mock = normalizeMock(stored as never);
    expect(mock.data).toHaveLength(1);
    expect(mock.fallback?.status).toBe(418);
    expect(mock.routes[0]).toMatchObject({
      kind: "crud",
      bucket: "b",
      id_key: "uid",
      responses: [],
    });
    expect(mock.routes[1]?.responses[0]?.callbacks).toHaveLength(1);
    expect(mock.routes[1]).toMatchObject({ kind: "http", id_key: "id" });
    expect(endpointLabel(mock.routes[0]!)).toBe("DATA /users");
  });
});
