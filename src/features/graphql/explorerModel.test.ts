import { readFileSync } from "node:fs";
import { buildSchema } from "graphql";
import { describe, expect, it } from "vitest";
import { membersOf, relatedTypes, rootTypes, searchSchema } from "./explorerModel";

const schema = buildSchema(readFileSync("src/test/graphql/schema.graphql", "utf8"));
const type = (name: string) => schema.getType(name)!;

describe("explorerModel", () => {
  it("lists the root types that exist", () => {
    expect(rootTypes(schema).map((r) => r.label)).toContain("Query");
    expect(rootTypes(schema).every((r) => schema.getType(r.name))).toBe(true);
  });

  it("describes fields with arguments, types and deprecation", () => {
    const user = membersOf(type("User")).find((m) => m.name === "posts")!;
    expect(user).toMatchObject({ typeText: "[Post!]!", typeName: "Post" });
    expect(user.args).toEqual([{ name: "first", typeText: "Int", typeName: "Int" }]);
    const author = membersOf(type("Post")).find((m) => m.name === "author")!;
    expect(author.deprecation).toBe("Use `writer` instead.");
  });

  it("lists enum values and input fields, and nothing for scalars and unions", () => {
    expect(membersOf(type("Role")).map((m) => m.name)).toEqual(["ADMIN", "MEMBER", "GUEST"]);
    expect(membersOf(type("CreateUserInput")).map((m) => m.name)).toEqual(["name", "role", "email"]);
    expect(membersOf(type("String"))).toEqual([]);
    expect(membersOf(type("SearchResult"))).toEqual([]);
  });

  it("relates unions and interfaces", () => {
    expect(relatedTypes(type("SearchResult"), schema)).toEqual({ label: "Possible types", names: ["User", "Post"] });
    expect(relatedTypes(type("User"), schema)).toEqual({ label: "Implements", names: ["Node"] });
    expect(relatedTypes(type("Node"), schema)?.label).toBe("Implemented by");
    expect(relatedTypes(type("Role"), schema)).toBeNull();
  });

  it("searches type and field names, ignoring case and introspection types", () => {
    expect(searchSchema(schema, "")).toEqual([]);
    const hits = searchSchema(schema, "WRITER");
    expect(hits).toEqual([{ typeName: "Post", member: "writer" }]);
    expect(searchSchema(schema, "user")).toContainEqual({ typeName: "User", member: null });
    expect(searchSchema(schema, "schema").some((h) => h.typeName.startsWith("__"))).toBe(false);
  });
});
