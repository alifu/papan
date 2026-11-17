import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SubprotocolsEditor } from "./SubprotocolsEditor";

describe("SubprotocolsEditor", () => {
  it("lists subprotocols and removes one", async () => {
    const onChange = vi.fn();
    render(<SubprotocolsEditor value={["a", "b"]} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Remove a" }));
    expect(onChange).toHaveBeenCalledWith(["b"]);
  });

  it("adds a valid token on Enter and clears the field", async () => {
    const onChange = vi.fn();
    render(<SubprotocolsEditor value={["a"]} onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("New subprotocol"), "chat.v2{Enter}");
    expect(onChange).toHaveBeenCalledWith(["a", "chat.v2"]);
    expect(screen.getByLabelText("New subprotocol")).toHaveValue("");
  });

  it("rejects an invalid or duplicate token with a message", async () => {
    const onChange = vi.fn();
    render(<SubprotocolsEditor value={["a"]} onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("New subprotocol"), "bad value");
    expect(screen.getByRole("alert")).toHaveTextContent(/token/);
    expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();
    await userEvent.clear(screen.getByLabelText("New subprotocol"));
    await userEvent.type(screen.getByLabelText("New subprotocol"), "a");
    expect(screen.getByRole("alert")).toHaveTextContent(/already/);
    expect(onChange).not.toHaveBeenCalled();
  });
});
