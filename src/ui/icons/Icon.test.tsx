import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { Icon } from "./Icon";
import { GLYPHS } from "./glyphs";
import { ICON_NAMES, type IconName } from "./names";

const EXPECTED_NAMES: readonly IconName[] = [
  "folder", "folder-open", "request-http", "request-graphql", "request-grpc", "request-soap",
  "request-ws", "webhook", "environment", "git", "mock", "journey", "play", "stop", "plus",
  "close", "settings", "sun", "moon", "monitor", "chevron-left", "chevron-right", "chevron-down",
  "search", "history",
];

const DEFAULT_SIZE = "16";
const LARGE_SIZE = "24";

function renderSvg(name: IconName, extra: { size?: 16 | 24; title?: string; className?: string } = {}) {
  const { container } = render(<Icon name={name} {...extra} />);
  const svg = container.querySelector("svg");
  if (!svg) {
    throw new Error(`Icon "${name}" rendered no <svg>`);
  }
  return svg;
}

describe("icon names", () => {
  it("lists exactly the 25 contract names", () => {
    expect([...ICON_NAMES]).toEqual(EXPECTED_NAMES);
  });

  it("has a glyph for every name and no others", () => {
    expect(Object.keys(GLYPHS).sort()).toEqual([...ICON_NAMES].sort());
  });
});

describe("Icon markup", () => {
  it.each(ICON_NAMES)("renders a 16-grid svg for %s", (name) => {
    const svg = renderSvg(name);

    expect(svg.tagName.toLowerCase()).toBe("svg");
    expect(svg.getAttribute("viewBox")).toBe("0 0 16 16");
    expect(svg.children.length).toBeGreaterThan(0);
  });

  it.each(ICON_NAMES)("contains no id attribute for %s", (name) => {
    const { container } = render(<Icon name={name} />);

    expect(container.querySelectorAll("[id]")).toHaveLength(0);
  });

  it("defaults to 16px", () => {
    const svg = renderSvg("folder");

    expect(svg.getAttribute("width")).toBe(DEFAULT_SIZE);
    expect(svg.getAttribute("height")).toBe(DEFAULT_SIZE);
  });

  it("renders at 24px when asked", () => {
    const svg = renderSvg("folder", { size: 24 });

    expect(svg.getAttribute("width")).toBe(LARGE_SIZE);
    expect(svg.getAttribute("height")).toBe(LARGE_SIZE);
  });

  it("is hidden from assistive tech without a title", () => {
    const svg = renderSvg("play");

    expect(svg.getAttribute("aria-hidden")).toBe("true");
    expect(svg.getAttribute("role")).toBeNull();
    expect(svg.querySelector("title")).toBeNull();
  });

  it("exposes role img with a title child when titled", () => {
    renderSvg("play", { title: "Send request" });

    const img = screen.getByRole("img", { name: "Send request" });
    expect(img.getAttribute("aria-hidden")).toBeNull();
    expect(img.querySelector("title")?.textContent).toBe("Send request");
  });

  it("appends className to the base icon class", () => {
    const svg = renderSvg("stop", { className: "toolbar-icon" });

    expect(svg.getAttribute("class")).toBe("icon toolbar-icon");
  });
});
