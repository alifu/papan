import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import hostile from "../../test/responses/hostile.html?raw";
import { HtmlPreview, withPreviewCsp } from "./HtmlPreview";
import source from "./HtmlPreview.tsx?raw";

describe("HtmlPreview", () => {
  it("renders a fully sandboxed iframe", () => {
    const { container } = render(<HtmlPreview html={hostile} />);
    const frame = container.querySelector("iframe");
    expect(frame).not.toBeNull();
    expect(frame?.getAttribute("sandbox")).toBe("");
    expect(frame).toHaveAttribute("title", "Response preview");
  });
  it("prefixes the CSP meta in srcDoc", () => {
    const { container } = render(<HtmlPreview html="<p>x</p>" />);
    const doc = container.querySelector("iframe")?.getAttribute("srcdoc") ?? "";
    expect(doc.startsWith('<meta http-equiv="Content-Security-Policy"')).toBe(true);
    expect(doc).toContain("default-src 'none'");
    expect(doc.endsWith("<p>x</p>")).toBe(true);
    expect(withPreviewCsp("")).toContain("form-action 'none'");
  });
  it("puts no script in the parent document for hostile HTML", () => {
    const { container } = render(<HtmlPreview html={hostile} />);
    expect(container.querySelector("script")).toBeNull();
    expect(document.querySelector("script")).toBeNull();
  });
  it("never grants sandbox permissions or uses innerHTML in its source", () => {
    const src = source;
    expect(src).not.toMatch(/allow-/);
    expect(src).not.toMatch(/innerHTML|dangerouslySetInnerHTML/);
  });
});
