import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Dialog } from "./Dialog";

function Harness({ onClose = () => {} }: { onClose?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button onClick={() => setOpen(true)}>Open</button>
      <Dialog
        open={open}
        title="Rename"
        onClose={() => {
          onClose();
          setOpen(false);
        }}
        actions={<button>OK</button>}
      >
        <input aria-label="Name" />
      </Dialog>
    </>
  );
}

describe("Dialog", () => {
  it("is labelled by its title and modal", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    const dialog = screen.getByRole("dialog", { name: "Rename" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
  });

  it("renders nothing when closed", () => {
    render(<Dialog open={false} title="X" onClose={() => {}}>b</Dialog>);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("moves focus in and restores it on close", async () => {
    render(<Harness />);
    const opener = screen.getByRole("button", { name: "Open" });
    await userEvent.click(opener);
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(opener).toHaveFocus();
  });

  it("wraps Tab in both directions", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    await userEvent.tab();
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await userEvent.tab();
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("closes with Cancel and Escape", async () => {
    const onClose = vi.fn();
    render(<Harness onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(2);
  });
  it("adds an extra class to the dialog frame and keeps the base class", () => {
    render(
      <Dialog open title="Wide" onClose={() => {}} className="dialog-wide">
        <p>content</p>
      </Dialog>,
    );

    const frame = screen.getByRole("dialog");

    expect(frame).toHaveClass("dialog", "dialog-wide");
  });
});
