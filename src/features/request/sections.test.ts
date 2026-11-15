import { describe, expect, it } from "vitest";
import { sectionsFor } from "./sections";

const ids = (kind: "http" | "graphql" | "soap" | "ws") => sectionsFor(kind, { params: 0, headers: 0 }).map((s) => s.id);

describe("sectionsFor", () => {
  it("matches the plan per kind", () => {
    expect(ids("http")).toEqual(["params", "headers", "body", "auth", "scripts", "settings"]);
    expect(ids("graphql")).toEqual(["params", "headers", "query", "variables", "auth", "scripts", "settings"]);
    expect(ids("soap")).toEqual(["params", "headers", "body", "soap", "auth", "scripts", "settings"]);
    expect(ids("ws")).toEqual(["params", "headers", "messages", "subprotocols", "auth", "settings"]);
  });
});
