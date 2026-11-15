import { describe, expect, it } from "vitest";
import {
  draftProblem,
  endpointLabel,
  newMock,
  newRoute,
  removeAt,
  replaceAt,
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
