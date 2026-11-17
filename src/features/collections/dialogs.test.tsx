import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "./ConfirmDialog";
import { NameDialog } from "./NameDialog";

describe("NameDialog", () => {
  it("shows a validation message and does not submit an invalid name", async () => {
    const onSubmit = vi.fn().mockResolvedValue(true);
    render(<NameDialog title="New Folder" onSubmit={onSubmit} onClose={() => {}} />);

    await userEvent.click(screen.getByRole("button", { name: "OK" }));

    expect(screen.getByText("Name is required.")).toBeVisible();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("submits the trimmed name and closes on success", async () => {
    const onSubmit = vi.fn().mockResolvedValue(true);
    const onClose = vi.fn();
    render(<NameDialog title="Rename" initialName="Old" submitLabel="Rename" onSubmit={onSubmit} onClose={onClose} />);

    await userEvent.clear(screen.getByLabelText("Name"));
    await userEvent.type(screen.getByLabelText("Name"), "  New  {Enter}");

    expect(onSubmit).toHaveBeenCalledWith("New", "http");
    expect(onClose).toHaveBeenCalled();
  });

  it("stays open when the submit fails", async () => {
    const onClose = vi.fn();
    render(<NameDialog title="X" onSubmit={() => Promise.resolve(false)} onClose={onClose} />);
    await userEvent.type(screen.getByLabelText("Name"), "a{Enter}");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("Cancel closes without submitting", async () => {
    const onSubmit = vi.fn();
    const onClose = vi.fn();
    render(<NameDialog title="X" onSubmit={onSubmit} onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

describe("ConfirmDialog", () => {
  it("confirms and cancels", async () => {
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(<ConfirmDialog title="Delete" message="Sure?" onConfirm={onConfirm} onClose={onClose} />);

    expect(screen.getByText("Sure?")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
