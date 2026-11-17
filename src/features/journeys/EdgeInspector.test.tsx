import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { JourneyEdge } from "../../ipc/bindings";
import { EdgeInspector } from "./EdgeInspector";

const show = (edge: JourneyEdge) => {
  const onChange = vi.fn();
  render(
    <EdgeInspector
      edge={edge}
      fromLabel="Login"
      toLabel="Whoami"
      onChange={onChange}
    />,
  );
  return onChange;
};

describe("EdgeInspector", () => {
  it("names the edge by the two nodes it joins", () => {
    show({ id: "e1", from: "a", to: "b" });
    expect(screen.getByRole("heading")).toHaveTextContent("Login → Whoami");
  });

  it("adds a mapping", async () => {
    const onChange = show({ id: "e1", from: "a", to: "b" });
    await userEvent.click(screen.getByRole("button", { name: "Add mapping" }));
    expect(onChange).toHaveBeenCalledWith({
      map: [{ name: "", source: "body", path: "" }],
    });
  });

  it("edits and removes a mapping", async () => {
    const edge: JourneyEdge = {
      id: "e1",
      from: "a",
      to: "b",
      map: [{ name: "auth", source: "body", path: "data.token" }],
    };
    const onChange = show(edge);
    fireEvent.change(screen.getByLabelText("Mapping 1 variable"), {
      target: { value: "tok" },
    });
    expect(onChange).toHaveBeenLastCalledWith({
      map: [{ name: "tok", source: "body", path: "data.token" }],
    });
    await userEvent.click(
      screen.getByRole("button", { name: "Remove mapping 1" }),
    );
    expect(onChange).toHaveBeenLastCalledWith({ map: [] });
  });

  it("has no path for a status mapping", () => {
    show({
      id: "e1",
      from: "a",
      to: "b",
      map: [{ name: "code", source: "status", path: "" }],
    });
    expect(screen.queryByLabelText("Mapping 1 path")).toBeNull();
  });
});
