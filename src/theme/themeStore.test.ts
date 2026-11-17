import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "../ui/memoryStorage";
import { installMatchMedia } from "./matchMediaMock";
import {
  THEME_MODES,
  THEME_STORAGE_KEY,
  loadThemeMode,
  readSystemTheme,
  resolveTheme,
  useThemeStore,
  type ResolvedTheme,
  type ThemeMode,
} from "./themeStore";

describe("themeStore", () => {
  let storage: Storage;

  beforeEach(() => {
    storage = installMemoryStorage();
    useThemeStore.setState(useThemeStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to system mode", () => {
    expect(useThemeStore.getState().mode).toBe("system");
    expect(THEME_MODES).toEqual(["system", "light", "dark"]);
  });

  it("setMode updates state and persists", () => {
    useThemeStore.getState().setMode("dark");

    expect(useThemeStore.getState().mode).toBe("dark");
    expect(storage.getItem(THEME_STORAGE_KEY)).toBe("dark");
  });

  it("setSystemTheme updates the system theme", () => {
    useThemeStore.getState().setSystemTheme("dark");
    expect(useThemeStore.getState().systemTheme).toBe("dark");
  });

  it("loadThemeMode reads a valid stored mode", () => {
    storage.setItem(THEME_STORAGE_KEY, "light");
    expect(loadThemeMode()).toBe("light");
  });

  it.each([["bogus"], [null]])("loadThemeMode falls back for %s", (value) => {
    if (value !== null) storage.setItem(THEME_STORAGE_KEY, value);
    expect(loadThemeMode()).toBe("system");
  });

  it("loadThemeMode falls back when storage throws", () => {
    vi.spyOn(storage, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(loadThemeMode()).toBe("system");
  });

  it.each<[ThemeMode, ResolvedTheme, ResolvedTheme]>([
    ["system", "light", "light"],
    ["system", "dark", "dark"],
    ["light", "dark", "light"],
    ["light", "light", "light"],
    ["dark", "light", "dark"],
    ["dark", "dark", "dark"],
  ])("resolveTheme(%s, %s) is %s", (mode, system, expected) => {
    expect(resolveTheme(mode, system)).toBe(expected);
  });

  it("readSystemTheme follows matchMedia", () => {
    installMatchMedia(true);
    expect(readSystemTheme()).toBe("dark");
    installMatchMedia(false);
    expect(readSystemTheme()).toBe("light");
  });

  it("readSystemTheme is light without matchMedia", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(readSystemTheme()).toBe("light");
  });
});
