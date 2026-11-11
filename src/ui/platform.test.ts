import { afterEach, describe, expect, it, vi } from "vitest";
import { isMac, shortcutLabel } from "./platform";

function stubPlatform(platform: string): void {
  vi.stubGlobal("navigator", { platform });
}

describe("platform", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("detects macOS and uses the command symbol", () => {
    stubPlatform("MacIntel");
    expect(isMac()).toBe(true);
    expect(shortcutLabel("N")).toBe("⌘N");
  });

  it("uses Ctrl elsewhere", () => {
    stubPlatform("Win32");
    expect(isMac()).toBe(false);
    expect(shortcutLabel("N")).toBe("Ctrl+N");
  });
});
