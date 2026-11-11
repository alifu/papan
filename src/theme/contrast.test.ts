import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

// Vitest blanks CSS module imports (even ?raw), so read the file directly (vitest runs from the repo root).
const tokensCss = readFileSync("src/theme/tokens.css", "utf8");

const MIN_CONTRAST = 4.5;
const LIGHT_SELECTOR = ':root,\n:root[data-theme="light"] {';
const DARK_SELECTOR = ':root[data-theme="dark"] {';

type TokenMap = Readonly<Record<string, readonly string[]>>;

function blockAfter(selector: string): string {
  const start = tokensCss.indexOf(selector);
  if (start < 0) throw new Error(`missing ${selector}`);
  return tokensCss.slice(start, tokensCss.indexOf("}", start));
}

function parseTokens(block: string): TokenMap {
  const entries = [...block.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map(
    ([, name, value]) => [name, value.match(/#[0-9A-Fa-f]{6}/g) ?? []] as const,
  );
  return Object.fromEntries(entries);
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const PAIRS: ReadonlyArray<readonly [string, readonly string[]]> = [
  ["--color-text", ["--color-panel-bg", "--color-content-bg", "--color-status-bg", "--toolbar-bg", "--button-bg", "--button-hover-bg", "--button-active-bg"]],
  ["--color-text-muted", ["--color-panel-bg", "--color-content-bg"]],
  ["--color-header-text", ["--header-bg", "--color-tab-active-bg"]],
  ["--color-header-strong-text", ["--header-strong-bg"]],
  ["--color-selected-text", ["--color-selected-bg"]],
  ["--color-error-text", ["--color-status-bg"]],
];

const themes: ReadonlyArray<readonly [string, TokenMap]> = [
  ["light", parseTokens(blockAfter(LIGHT_SELECTOR))],
  ["dark", parseTokens(blockAfter(DARK_SELECTOR))],
];

describe("theme tokens", () => {
  it("define the same token names in both themes", () => {
    const [light, dark] = themes.map(([, tokens]) => Object.keys(tokens).sort());
    expect(light).toEqual(dark);
    expect(light.length).toBeGreaterThan(20);
  });

  describe.each(themes)("%s contrast", (_name, tokens) => {
    it.each(PAIRS.flatMap(([fg, bgs]) => bgs.map((bg) => [fg, bg] as const)))(
      "%s on %s meets AA at every gradient stop",
      (fg, bg) => {
        const [fgColor] = tokens[fg];
        expect(tokens[bg].length).toBeGreaterThan(0);
        for (const stop of tokens[bg]) {
          expect(contrast(fgColor, stop)).toBeGreaterThanOrEqual(MIN_CONTRAST);
        }
      },
    );
  });
});
