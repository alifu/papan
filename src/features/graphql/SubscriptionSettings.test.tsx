import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SubscriptionSettings } from "./SubscriptionSettings";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

describe("SubscriptionSettings", () => {
  it("edits the URL override and the init payload immutably", async () => {
    const onChange = vi.fn();
    const settings = { url: "", init_payload: "" };
    render(<SubscriptionSettings settings={settings} onChange={onChange} />);
    await userEvent.type(screen.getByLabelText("WebSocket URL"), "w");
    expect(onChange).toHaveBeenLastCalledWith({ url: "w", init_payload: "" });
    await userEvent.type(screen.getByLabelText("Connection init payload"), "x");
    expect(onChange).toHaveBeenLastCalledWith({ url: "", init_payload: "x" });
    expect(settings).toEqual({ url: "", init_payload: "" });
  });

  it("warns when the init payload is not a JSON object", () => {
    render(<SubscriptionSettings settings={{ url: "", init_payload: "[1]" }} onChange={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent("Connection init payload should be a JSON object.");
  });
});
