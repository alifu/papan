import { describe, expect, it } from "vitest";
import {
  applyRowsEdit,
  applyUrlEdit,
  joinQuery,
  parseQuery,
  rowsFromContent,
  splitUrl,
  type ParamRow,
} from "./params";

const names = (rows: readonly ParamRow[]) => rows.map((r) => `${r.enabled ? "+" : "-"}${r.name}=${r.value}`);

describe("splitUrl", () => {
  it("separates base, query and fragment", () => {
    expect(splitUrl("http://h/p?a=1#frag")).toEqual({ base: "http://h/p", query: "a=1", fragment: "#frag" });
  });
  it("reports no query when there is no question mark", () => {
    expect(splitUrl("http://h/p")).toEqual({ base: "http://h/p", query: null, fragment: "" });
    expect(splitUrl("http://h/p#x?y")).toEqual({ base: "http://h/p", query: null, fragment: "#x?y" });
  });
  it("ignores separators inside variable tokens", () => {
    const parts = splitUrl("{{base?x#y}}/p?q={{t#?}}#f");
    expect(parts).toEqual({ base: "{{base?x#y}}/p", query: "q={{t#?}}", fragment: "#f" });
  });
});

describe("parseQuery", () => {
  const cases: ReadonlyArray<[string, ReadonlyArray<[string, string]>]> = [
    ["a=1&b=2", [["a", "1"], ["b", "2"]]],
    ["flag", [["flag", ""]]],
    ["flag=", [["flag", ""]]],
    ["a=1&&b=2", [["a", "1"], ["b", "2"]]],
    ["=v", [["", "v"]]],
    ["a=b=c", [["a", "b=c"]]],
    ["a=1&a=2", [["a", "1"], ["a", "2"]]],
    ["a%20b=%26&c=x+y", [["a%20b", "%26"], ["c", "x+y"]]],
    ["q={{term}}&{{a&b}}=1", [["q", "{{term}}"], ["{{a&b}}", "1"]]],
    ["{{k=v}}", [["{{k=v}}", ""]]],
    ["", []],
  ];
  it.each(cases)("parses %j", (query, expected) => {
    expect(parseQuery(query).map((p) => [p.name, p.value])).toEqual(expected);
  });

  it.each(["flag=", "a=&b", "=", "a=1&=&b", "a=1&b=2", "flag&x=1", "=v", "a=b=c", "a%20b=%26&c=x+y", "q={{term}}&{{a&b}}=1", "a=1&a=2", ""])(
    "round-trips canonical query %j",
    (q) => {
      expect(joinQuery(parseQuery(q))).toBe(q);
    },
  );

  it("drops empty pieces when joined", () => {
    expect(joinQuery(parseQuery("a=1&&b=2"))).toBe("a=1&b=2");
  });
});

describe("rowsFromContent", () => {
  it("lists enabled rows from the URL then disabled rows", () => {
    const rows = rowsFromContent("http://h/?a=1", [{ name: "d", value: "9" }]);
    expect(names(rows)).toEqual(["+a=1", "-d=9"]);
  });
  it("gives no rows for a URL without query", () => {
    expect(rowsFromContent("http://h/", [])).toEqual([]);
  });
});

describe("applyUrlEdit", () => {
  const rows: readonly ParamRow[] = [
    { name: "a", value: "1", enabled: true },
    { name: "off", value: "x", enabled: false },
    { name: "b", value: "2", enabled: true },
  ];
  it("keeps disabled rows at their index", () => {
    expect(names(applyUrlEdit(rows, "h?a=1&z=3&b=2"))).toEqual(["+a=1", "-off=x", "+z=3", "+b=2"]);
  });
  it("clamps the index when the URL has fewer rows", () => {
    expect(names(applyUrlEdit(rows, "h"))).toEqual(["-off=x"]);
  });
  it("does not mutate its input", () => {
    const copy = JSON.stringify(rows);
    applyUrlEdit(rows, "h?q=1");
    expect(JSON.stringify(rows)).toBe(copy);
  });
});

describe("applyRowsEdit", () => {
  it("rewrites only the query, keeping base and fragment", () => {
    const rows = rowsFromContent("http://h/p?a=1#frag", []);
    const next = [...rows, { name: "b", value: "2", enabled: true }];
    expect(applyRowsEdit("http://h/p?a=1#frag", next).url).toBe("http://h/p?a=1&b=2#frag");
  });
  it("moves a disabled row out of the URL and back at its position", () => {
    const rows = rowsFromContent("h?a=1&b=2", []);
    const off = rows.map((r) => (r.name === "a" ? { ...r, enabled: false } : r));
    const edit = applyRowsEdit("h?a=1&b=2", off);
    expect(edit).toEqual({ url: "h?b=2", disabled_params: [{ name: "a", value: "1" }] });
    const on = off.map((r) => ({ ...r, enabled: true }));
    expect(applyRowsEdit(edit.url, on).url).toBe("h?a=1&b=2");
    expect(applyRowsEdit(edit.url, on).disabled_params).toEqual([]);
  });
  it("drops the question mark when no enabled row remains", () => {
    const rows = rowsFromContent("h?a=1", []).map((r) => ({ ...r, enabled: false }));
    expect(applyRowsEdit("h?a=1", rows).url).toBe("h");
  });
  it("ignores blank trailing rows", () => {
    const rows = [...rowsFromContent("h?a=1", []), { name: "", value: "", enabled: true }];
    expect(applyRowsEdit("h?a=1", rows).url).toBe("h?a=1");
  });
  it("adds a query to a URL that had none", () => {
    expect(applyRowsEdit("h#f", [{ name: "a", value: "", enabled: true }]).url).toBe("h?a#f");
  });
  it("does not mutate its input and leaves variable tokens alone", () => {
    const rows = rowsFromContent("{{base}}/x?q={{term}}", []);
    const copy = JSON.stringify(rows);
    expect(applyRowsEdit("{{base}}/x?q={{term}}", rows).url).toBe("{{base}}/x?q={{term}}");
    expect(JSON.stringify(rows)).toBe(copy);
  });
  it("splits a typed ampersand into two rows on the next URL edit", () => {
    const url = applyRowsEdit("h", [{ name: "a", value: "1&b=2", enabled: true }]).url;
    expect(names(applyUrlEdit([], url))).toEqual(["+a=1", "+b=2"]);
  });
});

describe("flag spelling is preserved", () => {
  it("does not rewrite another row's flag= when a different row is edited", () => {
    const rows = rowsFromContent("h?flag=&b=2", []);
    const edited = rows.map((r) => (r.name === "b" ? { ...r, value: "3" } : r));
    expect(applyRowsEdit("h?flag=&b=2", edited).url).toBe("h?flag=&b=3");
  });
  it("emits a new table row with an empty value as the bare name", () => {
    const rows = [...rowsFromContent("h?a=1", []), { name: "n", value: "", enabled: true }];
    expect(applyRowsEdit("h?a=1", rows).url).toBe("h?a=1&n");
  });
  it("keeps the = of a disabled flag= row out of disabled_params shape", () => {
    const rows = rowsFromContent("h?flag=", []).map((r) => ({ ...r, enabled: false }));
    expect(applyRowsEdit("h?flag=", rows).disabled_params).toEqual([{ name: "flag", value: "" }]);
  });
});
