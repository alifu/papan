import { describe, expect, it } from "vitest";
import type { SoapTemplate, WsdlRef } from "../../ipc/bindings";
import { emptyContent } from "../request/content";
import { applyTemplate, needsConfirmation } from "./applyTemplate";

const template: SoapTemplate = { version: "1.2", action: "urn:Add", address: "http://h/svc", envelope: "<e/>" };
const ref: WsdlRef = { url: "http://h/svc?wsdl", file: "", service: "S", port: "P", operation: "Add" };
const base = { ...emptyContent(), kind: "soap" as const };

describe("applyTemplate", () => {
  it("writes body, soap settings and an empty URL without mutating the input", () => {
    const next = applyTemplate(base, template, ref, false);
    expect(next.body).toEqual({ type: "xml", text: "<e/>" });
    expect(next.soap).toEqual({ version: "1.2", action: "urn:Add", wsdl: ref });
    expect(next.url).toBe("http://h/svc");
    expect(base.url).toBe("");
    expect(base.soap.wsdl).toBeNull();
  });

  it("keeps a typed URL unless replacing is confirmed", () => {
    const typed = { ...base, url: "http://mine" };
    expect(applyTemplate(typed, template, ref, false).url).toBe("http://mine");
    expect(applyTemplate(typed, template, ref, true).url).toBe("http://h/svc");
  });

  it("never blanks the URL when the template has no address", () => {
    const typed = { ...base, url: "http://mine" };
    expect(applyTemplate(typed, { ...template, address: "" }, ref, true).url).toBe("http://mine");
  });
});

describe("needsConfirmation", () => {
  it("is false for an untouched request", () => {
    expect(needsConfirmation(base, template)).toBe(false);
    expect(needsConfirmation({ ...base, body: { type: "xml", text: "  " } }, template)).toBe(false);
  });

  it("is false when regenerating the same envelope at the same address", () => {
    const same = { ...base, url: "http://h/svc", body: { type: "xml" as const, text: "<e/>" } };
    expect(needsConfirmation(same, template)).toBe(false);
  });

  it("is true for a different envelope, another body type or another URL", () => {
    expect(needsConfirmation({ ...base, body: { type: "xml", text: "<other/>" } }, template)).toBe(true);
    expect(needsConfirmation({ ...base, body: { type: "text", text: "x" } }, template)).toBe(true);
    expect(needsConfirmation({ ...base, url: "http://mine" }, template)).toBe(true);
  });
});
