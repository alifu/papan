import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Menu } from "./Menu";

function setup() {
  const onNew = vi.fn();
  const onClose = vi.fn();
  render(
    <div>
      <Menu label="File" items={[
        { label: "New Tab", shortcut: "Ctrl+N", onSelect: onNew },
        { label: "Close Tab", onSelect: onClose },
      ]} />
      <button>outside</button>
    </div>,
  );
  return { onNew, onClose, trigger: screen.getByRole("menuitem", { name: "File" }) };
}

describe("Menu", () => {
  it("opens on click with focus on the first item", async () => {
    const { trigger } = setup();
    await userEvent.click(trigger);

    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("menuitem", { name: /New Tab/ })).toHaveFocus();
    expect(screen.getByText("Ctrl+N")).toBeVisible();
  });

  it("wraps with the arrow keys", async () => {
    const { trigger } = setup();
    await userEvent.click(trigger);

    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Close Tab" })).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: /New Tab/ })).toHaveFocus();
    await userEvent.keyboard("{ArrowUp}");
    expect(screen.getByRole("menuitem", { name: "Close Tab" })).toHaveFocus();
  });

  it("selects with Enter and closes", async () => {
    const { trigger, onNew } = setup();
    await userEvent.click(trigger);
    await userEvent.keyboard("{Enter}");

    expect(onNew).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("selects with a click", async () => {
    const { trigger, onClose } = setup();
    await userEvent.click(trigger);
    await userEvent.click(screen.getByRole("menuitem", { name: "Close Tab" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and refocuses the trigger", async () => {
    const { trigger } = setup();
    await userEvent.click(trigger);
    await userEvent.keyboard("{Escape}");

    expect(screen.queryByRole("menu")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("closes on outside click and on Tab, ignores other keys", async () => {
    const { trigger } = setup();
    await userEvent.click(trigger);
    await userEvent.keyboard("x");
    expect(screen.getByRole("menu")).toBeVisible();
    await userEvent.click(screen.getByText("outside"));
    expect(screen.queryByRole("menu")).toBeNull();

    await userEvent.click(trigger);
    await userEvent.keyboard("{Tab}");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("toggles closed when the trigger is clicked again", async () => {
    const { trigger } = setup();
    await userEvent.click(trigger);
    await userEvent.click(trigger);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("supports an icon trigger named by its label and disabled items", async () => {
    const onSelect = vi.fn();
    const { container } = render(
      <Menu label="Actions" icon="chevron-down" className="extra"
        items={[{ label: "Off", disabled: true, onSelect }, { label: "On", onSelect }]} />,
    );
    const trigger = screen.getByRole("menuitem", { name: "Actions" });
    expect(trigger.querySelector("svg")).not.toBeNull();
    expect(container.firstChild).toHaveClass("menu", "extra");

    await userEvent.click(trigger);
    expect(screen.getByRole("menuitem", { name: "Off" })).toBeDisabled();
    await userEvent.click(screen.getByRole("menuitem", { name: "Off" }));
    expect(onSelect).not.toHaveBeenCalled();
  });
});
