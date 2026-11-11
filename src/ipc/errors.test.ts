import { describe, it, expect } from "vitest";
import { isAppError, toUserMessage } from "./errors";

const GENERIC_MESSAGE = "Something went wrong. Please try again.";

describe("isAppError", () => {
  it("accepts an error with a known code and a message", () => {
    expect(isAppError({ code: "not_found", message: "Missing." })).toBe(true);
  });

  it("accepts an error with string or null detail", () => {
    expect(isAppError({ code: "io", message: "m", detail: "d" })).toBe(true);
    expect(isAppError({ code: "io", message: "m", detail: null })).toBe(true);
  });

  it("rejects values that are not objects", () => {
    for (const value of [null, undefined, "not_found", 42, true]) {
      expect(isAppError(value)).toBe(false);
    }
  });

  it("rejects an unknown code", () => {
    expect(isAppError({ code: "nope", message: "m" })).toBe(false);
    expect(isAppError({ code: "toString", message: "m" })).toBe(false);
  });

  it("rejects a missing or non-string message", () => {
    expect(isAppError({ code: "io" })).toBe(false);
    expect(isAppError({ code: "io", message: 1 })).toBe(false);
  });

  it("rejects a non-string detail", () => {
    expect(isAppError({ code: "io", message: "m", detail: 5 })).toBe(false);
  });

  it("rejects a plain Error", () => {
    expect(isAppError(new Error("boom"))).toBe(false);
  });
});

describe("toUserMessage", () => {
  it("returns the backend message for an AppError", () => {
    const err = { code: "invalid_input", message: "login.http.yaml: url is required." };

    expect(toUserMessage(err)).toBe("login.http.yaml: url is required.");
  });

  it("never includes the technical detail", () => {
    const err = { code: "io", message: "A file operation failed.", detail: "EACCES /x" };

    expect(toUserMessage(err)).not.toContain("EACCES");
  });

  it("falls back to a message for the code when the message is blank", () => {
    expect(toUserMessage({ code: "not_found", message: "  " })).toBe(
      "The requested item could not be found.",
    );
  });

  it("returns a generic message for errors that are not AppErrors", () => {
    for (const err of [new Error("ECONNREFUSED"), "command x not found", null, undefined]) {
      expect(toUserMessage(err)).toBe(GENERIC_MESSAGE);
    }
  });
});
