import { describe, expect, it } from "vitest";
import { saveToVariableSnippet, type VariableScope } from "./saveToVariable";

type Row = readonly [path: readonly (string | number)[], name: string, scope: VariableScope, expected: string];

const rows: readonly Row[] = [
  [["data", "items", 0, "id"], "token", "env", 'pm.environment.set("token", pm.response.json().data.items[0].id);'],
  [[], "all", "env", 'pm.environment.set("all", pm.response.json());'],
  [["access-token"], "t", "collection", 'pm.collectionVariables.set("t", pm.response.json()["access-token"]);'],
  [["a b", "c"], "x", "global", 'pm.globals.set("x", pm.response.json()["a b"].c);'],
  [["1st"], "x", "env", 'pm.environment.set("x", pm.response.json()["1st"]);'],
  [["_ok", "$v"], "x", "env", 'pm.environment.set("x", pm.response.json()._ok.$v);'],
  [["q\"uote"], "x", "env", 'pm.environment.set("x", pm.response.json()["q\\"uote"]);'],
  [[0, 1], 'we"ird', "env", 'pm.environment.set("we\\"ird", pm.response.json()[0][1]);'],
  [[""], "x", "env", 'pm.environment.set("x", pm.response.json()[""]);'],
];

describe("saveToVariableSnippet", () => {
  it.each(rows)("builds the line for %j as %s in %s", (path, name, scope, expected) => {
    expect(saveToVariableSnippet(path, name, scope)).toBe(expected);
  });
});
