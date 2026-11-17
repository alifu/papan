import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "../ui/memoryStorage";
import { installMatchMedia } from "./matchMediaMock";
import { useThemeStore } from "./themeStore";
import { useApplyTheme } from "./useApplyTheme";

describe("useApplyTheme", () => {
  beforeEach(() => {
    installMemoryStorage();
    useThemeStore.setState(useThemeStore.getInitialState(), true);
    delete document.documentElement.dataset.theme;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("sets data-theme from the system setting", () => {
    installMatchMedia(true);
    renderHook(() => useApplyTheme());
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("follows system changes while mode is system", () => {
    const media = installMatchMedia(false);
    renderHook(() => useApplyTheme());
    act(() => media.setDark(true));
    expect(document.documentElement.dataset.theme).toBe("dark");
    act(() => media.setDark(false));
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("ignores system changes while mode is light", () => {
    const media = installMatchMedia(false);
    useThemeStore.getState().setMode("light");
    renderHook(() => useApplyTheme());
    act(() => media.setDark(true));
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("removes the listener on unmount", () => {
    const media = installMatchMedia(false);
    const { unmount } = renderHook(() => useApplyTheme());
    expect(media.listenerCount()).toBe(1);
    unmount();
    expect(media.listenerCount()).toBe(0);
  });

  it("falls back to light without matchMedia", () => {
    vi.stubGlobal("matchMedia", undefined);
    renderHook(() => useApplyTheme());
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});
