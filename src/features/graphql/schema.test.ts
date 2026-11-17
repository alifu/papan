import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { GraphqlSchema } from "../../ipc/bindings";
import { toSchema } from "./schema";

const read = (name: string) => readFileSync(`src/test/graphql/${name}`, "utf8");
const introspection: GraphqlSchema = {
  endpoint: "http://x/graphql",
  fetched_at_ms: 1,
  source: { type: "introspection", json: read("introspection.json") },
};
const sdl: GraphqlSchema = { endpoint: "", fetched_at_ms: null, source: { type: "sdl", text: read("schema.graphql") } };

describe("toSchema", () => {
  it("builds from an introspection result", () => {
    const schema = toSchema(introspection);
    expect(schema).not.toBeInstanceOf(Error);
    expect((schema as import("graphql").GraphQLSchema).getType("User")).toBeDefined();
  });

  it("builds from SDL", () => {
    const schema = toSchema(sdl);
    expect((schema as import("graphql").GraphQLSchema).getQueryType()?.name).toBe("Query");
  });

  it("memoises per schema object", () => {
    expect(toSchema(introspection)).toBe(toSchema(introspection));
  });

  it("returns the error for broken input", () => {
    const bad: GraphqlSchema = { ...sdl, source: { type: "sdl", text: "type {" } };
    expect(toSchema(bad)).toBeInstanceOf(Error);
    const badJson: GraphqlSchema = { ...introspection, source: { type: "introspection", json: "nope" } };
    expect(toSchema(badJson)).toBeInstanceOf(Error);
  });
});
