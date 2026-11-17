import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { Scripts } from "../../ipc/bindings";
import { ScriptsEditor, type ScriptPhase } from "./ScriptsEditor";

vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

function Harness({ onChange }: { onChange: (s: Scripts) => void }) {
  const [scripts, setScripts] = useState<Scripts>({ enabled: true, pre_request: "", post_response: "" });
  const [phase, setPhase] = useState<ScriptPhase>("pre_request");
  return (
    <ScriptsEditor
      scripts={scripts}
      phase={phase}
      onPhase={setPhase}
      onChange={(s) => {
        setScripts(s);
        onChange(s);
      }}
    />
  );
}

describe("ScriptsEditor", () => {
  it("edits the selected phase only", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Post-response" }));
    await userEvent.type(screen.getByLabelText("Post-response script"), "x");
    expect(onChange).toHaveBeenLastCalledWith({ enabled: true, pre_request: "", post_response: "x" });
  });

  it("appends a snippet to the active script", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Parse JSON response" }));
    expect(onChange).toHaveBeenLastCalledWith({
      enabled: true,
      pre_request: "const data = pm.response.json();\n",
      post_response: "",
    });
  });

  it("toggles Enabled and warns while disabled", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    expect(screen.queryByRole("note")).toBeNull();
    await userEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
    expect(onChange).toHaveBeenLastCalledWith({ enabled: false, pre_request: "", post_response: "" });
    expect(screen.getByRole("note")).toHaveTextContent(
      "Disabled: these scripts will not run. Review them before enabling.",
    );
    await userEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
    expect(screen.queryByRole("note")).toBeNull();
  });
});
