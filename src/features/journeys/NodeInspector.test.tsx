import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { JourneyNode, TreeNode } from "../../ipc/bindings";
import { NodeInspector } from "./NodeInspector";

const requests: TreeNode[] = [
  {
    path: "auth/login.http.yaml",
    name: "login",
    kind: "request",
    method: "POST",
    request_kind: "http",
    children: [],
  },
];

const show = (node: JourneyNode, onChange = vi.fn()) => {
  render(<NodeInspector node={node} requests={requests} onChange={onChange} />);
  return onChange;
};

describe("NodeInspector", () => {
  it("picks a request for a Request node", async () => {
    const onChange = show({ id: "n2", type: "request", request: "" });
    await userEvent.selectOptions(
      screen.getByLabelText("Request"),
      "auth/login.http.yaml",
    );
    expect(onChange).toHaveBeenCalledWith({ request: "auth/login.http.yaml" });
  });

  it("flags a request that is no longer in the collection", () => {
    show({ id: "n2", type: "request", request: "gone.http.yaml" });
    expect(
      screen.getByRole("option", { name: "gone.http.yaml (missing)" }),
    ).toBeInTheDocument();
  });

  it("adds and edits an output", async () => {
    const onChange = show({
      id: "n2",
      type: "request",
      request: "",
      outputs: [],
    });
    await userEvent.click(screen.getByRole("button", { name: "Add output" }));
    expect(onChange).toHaveBeenCalledWith({
      outputs: [{ name: "", source: "body", path: "" }],
    });
  });

  it("edits a check; the name field appears for a body source", async () => {
    const onChange = show({
      id: "n3",
      type: "assert",
      check: { source: "body", name: "", op: "eq", value: "" },
    });
    await userEvent.type(screen.getByLabelText("JSON path"), "a");
    expect(onChange).toHaveBeenLastCalledWith({
      check: { source: "body", name: "a", op: "eq", value: "" },
    });
  });

  it("hides the name field for a status source", () => {
    show({
      id: "n3",
      type: "condition",
      check: { source: "status", op: "eq", value: "200" },
    });
    expect(screen.queryByLabelText("Name")).toBeNull();
  });

  it("turns the items textarea into a list", async () => {
    const onChange = show({
      id: "n4",
      type: "loop",
      count: 3,
      loop_var: "item",
    });
    fireEvent.change(screen.getByLabelText("Items (one per line)"), {
      target: { value: "a\nb" },
    });
    expect(onChange).toHaveBeenLastCalledWith({ items: ["a", "b"] });
  });

  it("sets and clears a per-node failure policy", async () => {
    const onChange = show({ id: "n2", type: "script", script: "" });
    await userEvent.selectOptions(
      screen.getByLabelText("When this node fails"),
      "continue",
    );
    expect(onChange).toHaveBeenCalledWith({ on_failure: "continue" });
    await userEvent.selectOptions(
      screen.getByLabelText("When this node fails"),
      "",
    );
    expect(onChange).toHaveBeenLastCalledWith({ on_failure: null });
  });

  it("offers no failure policy on Start and End", () => {
    show({ id: "n1", type: "start" });
    expect(screen.queryByLabelText("When this node fails")).toBeNull();
  });

  it("sets a data file and then leaves the items alone", async () => {
    const onChange = show({ id: "n4", type: "loop", data_file: "data/u.csv" });
    expect(screen.getByLabelText("Items (one per line)")).toBeDisabled();
    fireEvent.change(screen.getByLabelText(/Data file/), {
      target: { value: "data/v.csv" },
    });
    expect(onChange).toHaveBeenLastCalledWith({ data_file: "data/v.csv" });
  });
});
