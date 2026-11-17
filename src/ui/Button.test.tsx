import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";

describe("Button", () => {
  it("renders its label and icon and handles clicks", async () => {
    const onClick = vi.fn();
    const { container } = render(<Button label="New Tab" icon="plus" onClick={onClick} />);

    await userEvent.click(screen.getByRole("button", { name: "New Tab" }));

    expect(onClick).toHaveBeenCalledTimes(1);
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("exposes aria-pressed", () => {
    render(<Button aria-label="Toggle" pressed />);
    expect(screen.getByRole("button", { name: "Toggle" })).toHaveAttribute("aria-pressed", "true");
  });

  it("omits aria-pressed and icon when not given, and merges classes", () => {
    const { container } = render(<Button label="Plain" className="extra" />);
    const button = screen.getByRole("button", { name: "Plain" });
    expect(button).not.toHaveAttribute("aria-pressed");
    expect(button).toHaveClass("btn", "extra");
    expect(container.querySelector("svg")).toBeNull();
  });
});
