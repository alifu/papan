import { describe, expect, it } from "vitest";
import type { PostmanFileSummary, PostmanPreview, ReportItem } from "../../ipc/bindings";
import {
  defaultChoices,
  filterReport,
  groupReport,
  importBlocker,
  needsCollection,
  reportCounts,
  reportToText,
  secretCandidates,
  toggleName,
  toImportOptions,
} from "./postmanModel";

const file = (over: Partial<PostmanFileSummary>): PostmanFileSummary => ({
  file: "a.json",
  kind: "collection_v21",
  name: "A",
  requests: 1,
  folders: 0,
  variables: 0,
  scripts: 0,
  secret_candidates: [],
  ...over,
});
const preview = (files: PostmanFileSummary[]): PostmanPreview => ({ token: "t", files, report: [], truncated: 0 });
const item = (level: ReportItem["level"], f: string, message: string, path = ""): ReportItem => ({
  level,
  file: f,
  path,
  message,
});

describe("postmanModel", () => {
  it.each([
    [["collection_v21"], false],
    [["collection_v20", "environment"], false],
    [["environment"], true],
    [["globals", "environment"], true],
    [[], true],
  ] as const)("needsCollection for %j is %s", (kinds, expected) => {
    expect(needsCollection(preview(kinds.map((kind) => file({ kind }))))).toBe(expected);
  });

  it("collects secret candidates sorted and unique", () => {
    const p = preview([file({ secret_candidates: ["token", "apiKey"] }), file({ secret_candidates: ["token"] })]);
    expect(secretCandidates(p)).toEqual(["apiKey", "token"]);
  });

  it("defaults: secrets ticked, nothing stored, new collection when a collection is present", () => {
    const p = preview([file({ secret_candidates: ["token"] })]);
    expect(defaultChoices(p, ["c1"])).toEqual({
      target: "new_collection",
      collectionId: "c1",
      secretNames: ["token"],
      storeSecretValues: false,
      importScripts: true,
      globalsClash: "keep",
    });
  });

  it("defaults to the open collection for environment-only previews", () => {
    expect(defaultChoices(preview([file({ kind: "environment" })]), ["c1"]).target).toBe("collection");
  });

  it("blocks env-only import into a new collection or with no open collection", () => {
    const p = preview([file({ kind: "environment" })]);
    expect(importBlocker(p, { ...defaultChoices(p, ["c1"]), target: "new_collection" })).toMatch(/open collection/);
    expect(importBlocker(p, defaultChoices(p, []))).toMatch(/Open a collection/);
    expect(importBlocker(p, defaultChoices(p, ["c1"]))).toBeNull();
    expect(importBlocker(preview([]), defaultChoices(preview([]), []))).toBe("Nothing to import.");
  });

  it("builds options for both targets", () => {
    const p = preview([file({})]);
    const base = defaultChoices(p, ["c1"]);
    expect(toImportOptions(base).target).toEqual({ type: "new_collection" });
    expect(toImportOptions({ ...base, target: "collection" }).target).toEqual({
      type: "collection",
      collection_id: "c1",
    });
    expect(toImportOptions({ ...base, secretNames: ["x"] }).secret_names).toEqual(["x"]);
  });

  it("toggles names without mutating", () => {
    const names = ["a"];
    expect(toggleName(names, "b")).toEqual(["a", "b"]);
    expect(toggleName(names, "a")).toEqual([]);
    expect(names).toEqual(["a"]);
  });

  it("counts, filters and groups the report", () => {
    const report = [item("info", "a", "i1"), item("warning", "b", "w1"), item("skipped", "a", "s1")];
    expect(reportCounts(report)).toEqual({ skipped: 1, warning: 1, info: 1 });
    expect(filterReport(report, "warning")).toEqual([report[1]]);
    expect(filterReport(report, "all")).toBe(report);
    expect(groupReport(report).map((g) => [g.file, g.items.length])).toEqual([["a", 2], ["b", 1]]);
  });

  it("renders the report as text", () => {
    const text = reportToText([item("warning", "a.json", "bad", "$.item[0]"), item("info", "a.json", "ok")]);
    expect(text).toBe("a.json\n  [warning] $.item[0]: bad\n  [info] ok");
  });
});
