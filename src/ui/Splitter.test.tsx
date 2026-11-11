import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Splitter } from "./Splitter";

function setup() {
  const onResize = vi.fn();
  const onToggle = vi.fn();
  render(
    <Splitter value={260} min={180} max={480} step={16} label="Resize sidebar"
      onResize={onResize} onToggle={onToggle} />,
  );
  return { onResize, onToggle, separator: screen.getByRole("separator", { name: "Resize sidebar" }) };
}

describe("Splitter", () => {
  it("exposes separator semantics", () => {
    const { separator } = setup();
    expect(separator).toHaveAttribute("aria-orientation", "vertical");
    expect(separator).toHaveAttribute("aria-valuenow", "260");
    expect(separator).toHaveAttribute("aria-valuemin", "180");
    expect(separator).toHaveAttribute("aria-valuemax", "480");
    expect(separator).toHaveAttribute("tabindex", "0");
  });

  it("resizes by start width plus pointer delta until pointer up", () => {
    const { onResize, separator } = setup();

    fireEvent.pointerDown(separator, { clientX: 100, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 140 });
    fireEvent.pointerUp(window);
    fireEvent.pointerMove(window, { clientX: 300 });

    expect(onResize).toHaveBeenCalledTimes(1);
    expect(onResize).toHaveBeenCalledWith(300);
  });

  it("removes window listeners when unmounted mid-drag", () => {
    const onResize = vi.fn();
    const { unmount } = render(
      <Splitter value={260} min={180} max={480} step={16} label="s" onResize={onResize} onToggle={vi.fn()} />,
    );
    fireEvent.pointerDown(screen.getByRole("separator"), { clientX: 0, pointerId: 1 });
    unmount();
    fireEvent.pointerMove(window, { clientX: 50 });
    expect(onResize).not.toHaveBeenCalled();
  });

  it.each([
    ["ArrowLeft", 244],
    ["ArrowRight", 276],
    ["Home", 180],
    ["End", 480],
  ])("%s resizes to %s", (key, expected) => {
    const { onResize, separator } = setup();
    fireEvent.keyDown(separator, { key });
    expect(onResize).toHaveBeenCalledWith(expected);
  });

  it("Enter toggles and other keys are ignored", () => {
    const { onResize, onToggle, separator } = setup();
    fireEvent.keyDown(separator, { key: "Enter" });
    fireEvent.keyDown(separator, { key: "a" });
    expect(onToggle).toHaveBeenCalledTimes(1);
    expect(onResize).not.toHaveBeenCalled();
  });
});
