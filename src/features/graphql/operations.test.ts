import { describe, expect, it } from "vitest";
import { emptyContent } from "../request/content";
import { listOperations, runsSubscription, selectedOperation } from "./operations";

const TWO = "query A { a } mutation B { b } subscription C { c }";

describe("listOperations", () => {
  it.each([
    ["", []],
    ["query {", []],
    ["{ me }", [{ name: "", type: "query" }]],
    ["fragment F on User { id } query Q { me { ...F } }", [{ name: "Q", type: "query" }]],
    [TWO, [
      { name: "A", type: "query" },
      { name: "B", type: "mutation" },
      { name: "C", type: "subscription" },
    ]],
  ])("lists %j", (query, expected) => {
    expect(listOperations(query)).toEqual(expected);
  });
});

describe("selectedOperation", () => {
  it("picks the named operation", () => {
    expect(selectedOperation(TWO, "B")).toEqual({ name: "B", type: "mutation" });
  });
  it("picks the only operation when no name is set", () => {
    expect(selectedOperation("query A { a }", "")).toEqual({ name: "A", type: "query" });
  });
  it("falls back to the only operation when the name is stale", () => {
    expect(selectedOperation("subscription S { s }", "B")).toEqual({ name: "S", type: "subscription" });
  });
  it("is null when ambiguous, unknown or unparsable", () => {
    expect(selectedOperation(TWO, "")).toBeNull();
    expect(selectedOperation(TWO, "Z")).toBeNull();
    expect(selectedOperation("query {", "")).toBeNull();
  });
});

describe("runsSubscription", () => {
  const gql = (query: string, operation_name: string) => ({
    ...emptyContent(),
    kind: "graphql" as const,
    body: { type: "graphql" as const, query, variables: "", operation_name },
  });
  it("is true only for a selected subscription", () => {
    expect(runsSubscription(gql(TWO, "C"))).toBe(true);
    expect(runsSubscription(gql(TWO, "A"))).toBe(false);
    expect(runsSubscription(gql("subscription S { s }", ""))).toBe(true);
  });
  it("is false for other kinds", () => {
    expect(runsSubscription(emptyContent())).toBe(false);
  });
});
