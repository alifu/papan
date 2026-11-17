import { describe, expect, it } from "vitest";
import shared from "../../test/vars/template-cases.json";
import { findTokens, unresolvedIn } from "./template";

const CASES: ReadonlyArray<{ text: string; tokens: ReadonlyArray<[number, number, string]> }> = [
  { text: "no variables", tokens: [] },
  { text: "{{a}}", tokens: [[0, 5, "a"]] },
  { text: "x{{a}}y{{b}}", tokens: [[1, 6, "a"], [7, 12, "b"]] },
  { text: "{{a}}{{b}}", tokens: [[0, 5, "a"], [5, 10, "b"]] },
  { text: "{{  spaced name  }}", tokens: [[0, 19, "spaced name"]] },
  { text: "{{}}", tokens: [] },
  { text: "{{   }}", tokens: [] },
  { text: "{{ a{b }}", tokens: [] },
  { text: "{{a", tokens: [] },
  { text: "{{{a}}", tokens: [[1, 6, "a"]] },
  { text: "{{a}}}", tokens: [[0, 5, "a"]] },
  { text: "\\{{x}}", tokens: [] },
  { text: "\\\\{{x}}", tokens: [] },
  { text: "a\\b{{x}}", tokens: [[3, 8, "x"]] },
  { text: "{{a\nb}}", tokens: [] },
  { text: "{{a\r}}", tokens: [] },
  { text: `{{${"n".repeat(128)}}}`, tokens: [[0, 132, "n".repeat(128)]] },
  { text: `{{${"n".repeat(129)}}}`, tokens: [] },
  { text: "é{{名前}}", tokens: [[1, 7, "名前"]] },
  { text: "{{$uuid}}", tokens: [[0, 9, "$uuid"]] },
];

describe("findTokens against the shared Rust/TS cases", () => {
  it.each(shared as ReadonlyArray<{ text: string; tokens: ReadonlyArray<{ from: number; to: number; name: string }> }>)(
    "tokenises $text",
    ({ text, tokens }) => {
      expect(findTokens(text)).toEqual(tokens);
    },
  );
});

describe("findTokens", () => {
  it.each(CASES)("tokenises $text", ({ text, tokens }) => {
    const found = findTokens(text).map((t) => [t.from, t.to, t.name]);
    expect(found).toEqual(tokens);
  });

  it("uses UTF-16 offsets", () => {
    const text = "😀{{a}}";
    const [token] = findTokens(text);
    expect(text.slice(token.from, token.to)).toBe("{{a}}");
  });

  it("counts the name length in code points", () => {
    const name = "😀".repeat(128);
    expect(findTokens(`{{${name}}}`)).toHaveLength(1);
  });

  it("never throws on odd input", () => {
    for (const text of ["{", "}}", "{{{{", "\\", "\\{{", "{{\\{{a}}", "}}{{"]) {
      expect(() => findTokens(text)).not.toThrow();
    }
  });
});

describe("unresolvedIn", () => {
  const variables = [
    { name: "a", resolved: true },
    { name: "off", resolved: false },
    { name: "$uuid", resolved: true },
  ];

  it("lists names no resolved variable defines, once each", () => {
    expect(unresolvedIn("{{a}} {{b}} {{off}} {{b}} {{$uuid}} {{$nope}}", variables)).toEqual([
      "b",
      "off",
      "$nope",
    ]);
  });

  it("returns nothing for text without tokens", () => {
    expect(unresolvedIn("plain \\{{a}}", variables)).toEqual([]);
  });
});
