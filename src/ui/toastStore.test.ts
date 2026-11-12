import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  TOAST_AUTO_DISMISS_MS,
  TOAST_MAX_VISIBLE,
  dismissToast,
  reportError,
  showToast,
  useToastStore,
} from "./toastStore";

const messages = () => useToastStore.getState().toasts.map((t) => t.message);

describe("toastStore", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useToastStore.setState({ toasts: [] });
  });
  afterEach(() => vi.useRealTimers());

  it("shows a toast without mutating the previous array", () => {
    const before = useToastStore.getState().toasts;
    showToast({ kind: "info", message: "hi" });
    expect(before).toHaveLength(0);
    expect(messages()).toEqual(["hi"]);
  });

  it("keeps only the newest toasts up to the cap", () => {
    for (let i = 0; i < TOAST_MAX_VISIBLE + 2; i++) showToast({ kind: "info", message: `m${i}` });
    expect(messages()).toEqual(["m2", "m3", "m4"]);
  });

  it("auto-dismisses after the timeout", () => {
    showToast({ kind: "info", message: "bye" });
    act(() => vi.advanceTimersByTime(TOAST_AUTO_DISMISS_MS - 1));
    expect(messages()).toEqual(["bye"]);
    act(() => vi.advanceTimersByTime(1));
    expect(messages()).toEqual([]);
  });

  it("dismisses manually and ignores unknown ids", () => {
    const id = showToast({ kind: "info", message: "a" });
    dismissToast(999);
    expect(messages()).toEqual(["a"]);
    dismissToast(id);
    expect(messages()).toEqual([]);
  });

  it("reportError uses the AppError message", () => {
    reportError({ code: "io", message: "Disk full" });
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "error", message: "Disk full" });
  });

  it("reportError uses a generic message for strings", () => {
    reportError("boom: stack trace");
    expect(messages()).toEqual(["Something went wrong. Please try again."]);
  });
});
