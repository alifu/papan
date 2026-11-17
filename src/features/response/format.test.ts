import { describe, expect, it } from "vitest";
import bigint from "../../test/responses/bigint.json?raw";
import feed from "../../test/responses/feed.xml?raw";
import { prettyJson, prettyXml } from "./format";

describe("prettyJson", () => {
  it("indents objects and arrays", () => {
    expect(prettyJson('{"a":[1,2],"b":{"c":null}}')).toBe(
      '{\n  "a": [\n    1,\n    2\n  ],\n  "b": {\n    "c": null\n  }\n}',
    );
  });
  it("keeps every digit of large integers", () => {
    const out = prettyJson(bigint) ?? "";
    for (const n of ["9007199254740993", "18446744073709551615", "-9007199254740993", "12345678901234567890"]) {
      expect(out).toContain(n);
    }
  });
  it("keeps duplicate keys, escapes and delimiters inside strings", () => {
    expect(prettyJson('{"a":1,"a":"x,\\"{:}"}')).toBe('{\n  "a": 1,\n  "a": "x,\\"{:}"\n}');
  });
  it("keeps empty containers compact", () => {
    expect(prettyJson('{"a":{},"b":[ ]}')).toBe('{\n  "a": {},\n  "b": []\n}');
  });
  it("returns null for invalid JSON", () => {
    expect(prettyJson("{oops")).toBeNull();
  });
});

describe("prettyXml", () => {
  it("indents nested elements and keeps the declaration", () => {
    const out = prettyXml('<?xml version="1.0"?><a><b x="1">t</b><c/></a>');
    expect(out).toBe('<?xml version="1.0"?>\n<a>\n  <b x="1">t</b>\n  <c/>\n</a>');
  });
  it("formats the feed fixture", () => {
    const out = prettyXml(feed) ?? "";
    expect(out).toContain("\n  <entry>\n    <id>urn:papan:1</id>");
  });
  it("escapes text and returns null for malformed XML", () => {
    expect(prettyXml("<a>1 &lt; 2</a>")).toBe("<a>1 &lt; 2</a>");
    expect(prettyXml("<a><b></a>")).toBeNull();
  });
});
