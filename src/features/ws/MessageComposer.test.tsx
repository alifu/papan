import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { WsFormat } from "../../ipc/bindings";
import { MessageComposer, type MessageComposerProps } from "./MessageComposer";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const setup = (over: Partial<MessageComposerProps> = {}) => {
  const props: MessageComposerProps = {
    format: "text",
    text: "",
    open: true,
    variables: [],
    templateNames: [],
    onFormat: vi.fn(),
    onText: vi.fn(),
    onSend: vi.fn(),
    onSaveTemplate: vi.fn(),
    ...over,
  };
  render(<MessageComposer {...props} />);
  return props;
};

describe("MessageComposer", () => {
  it("sends the current text in the selected format", async () => {
    const props = setup({ format: "json", text: '{"a":1}' });
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(props.onSend).toHaveBeenCalledWith({ frame: "json", text: '{"a":1}' });
  });

  it("disables Send until the connection is open", () => {
    setup({ open: false, text: "x" });
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("changes format", async () => {
    const props = setup();
    await userEvent.selectOptions(screen.getByLabelText("Format"), "binary" satisfies WsFormat);
    expect(props.onFormat).toHaveBeenCalledWith("binary");
  });

  it("warns about invalid JSON without blocking Send", () => {
    setup({ format: "json", text: "{oops" });
    expect(screen.getByRole("status")).toHaveTextContent("not valid JSON");
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("blocks Send for invalid base64", () => {
    setup({ format: "binary", text: "not base64!" });
    expect(screen.getByRole("alert")).toHaveTextContent("base64");
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("sends on Mod-Enter only when open and valid", async () => {
    const props = setup({ text: "hi" });
    await userEvent.type(screen.getByLabelText("Message"), "{Control>}{Enter}{/Control}");
    expect(props.onSend).toHaveBeenCalledWith({ frame: "text", text: "hi" });
  });

  it("saves a template under a valid name and offers Update for an existing one", async () => {
    const props = setup({ text: "hi", templateNames: ["Join"] });
    const save = screen.getByRole("button", { name: "Save as template" });
    expect(save).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Template name"), "Ping");
    await userEvent.click(save);
    expect(props.onSaveTemplate).toHaveBeenCalledWith("Ping");
    await userEvent.clear(screen.getByLabelText("Template name"));
    await userEvent.type(screen.getByLabelText("Template name"), "Join");
    expect(screen.getByRole("button", { name: "Update template" })).toBeEnabled();
  });
});
