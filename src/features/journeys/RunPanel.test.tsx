import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RunPanel } from "./RunPanel";
import { EMPTY_RUN, type RunState } from "./runState";

const node = { id: "n2", type: "request" as const, label: "Login" };
const done: RunState = {
  ...EMPTY_RUN,
  finished: "failed",
  steps: 3,
  message: "n2 failed",
  results: {
    n2: {
      message: "status 500",
      response: {
        status: 500,
        headers: [],
        body_preview: "boom",
        body_truncated: true,
      },
      vars: [{ name: "token", value: "abc" }],
      console: ["hi"],
    },
  },
};

describe("RunPanel", () => {
  it("says nothing ran yet", () => {
    render(<RunPanel run={EMPTY_RUN} node={null} />);
    expect(screen.getByRole("status")).toHaveTextContent("Not run yet.");
  });

  it("shows progress while running", () => {
    render(
      <RunPanel run={{ ...EMPTY_RUN, running: true, steps: 2 }} node={null} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Running… 2 steps");
  });

  it("shows the outcome and the selected node's response, variables and console", () => {
    render(<RunPanel run={done} node={node} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Run failed (3 steps): n2 failed",
    );
    expect(screen.getByText("500")).toBeVisible();
    expect(screen.getByLabelText("Response body")).toHaveTextContent("boom");
    expect(screen.getByLabelText("Response body")).toHaveTextContent("(cut)");
    expect(screen.getByLabelText("Variables set")).toHaveTextContent(
      "token = abc",
    );
    expect(screen.getByLabelText("Console")).toHaveTextContent("hi");
  });

  it("says when the selected node did not run", () => {
    render(<RunPanel run={{ ...done, results: {} }} node={node} />);
    expect(screen.getByText("This node did not run.")).toBeVisible();
  });
});
