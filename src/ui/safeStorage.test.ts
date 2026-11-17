import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMemoryStorage } from "./memoryStorage";
import { readStored, writeStored } from "./safeStorage";

describe("safeStorage", () => {
  let storage: Storage;

  beforeEach(() => {
    storage = installMemoryStorage();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("round-trips a value", () => {
    expect(writeStored("k", "v")).toBe(true);
    expect(readStored("k")).toBe("v");
  });

  it("returns null for a missing key", () => {
    expect(readStored("missing")).toBeNull();
  });

  it("returns null when getItem throws", () => {
    vi.spyOn(storage, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(readStored("k")).toBeNull();
  });

  it("returns false when setItem throws", () => {
    vi.spyOn(storage, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    expect(writeStored("k", "v")).toBe(false);
  });

  it("memory storage supports the full Storage surface", () => {
    storage.setItem("a", "1");
    expect(storage.length).toBe(1);
    expect(storage.key(0)).toBe("a");
    expect(storage.key(5)).toBeNull();
    storage.removeItem("a");
    storage.setItem("b", "2");
    storage.clear();
    expect(storage.length).toBe(0);
  });
});
