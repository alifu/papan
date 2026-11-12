import { act, render, screen } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { CodeEditor, type CodeEditorProps } from "./CodeEditor";

const base: CodeEditorProps = { value: "hello", language: "text", ariaLabel: "Body" };

const viewOf = (): EditorView => {
  const view = EditorView.findFromDOM(screen.getByLabelText("Body").closest(".cm-editor") as HTMLElement);
  if (!view) throw new Error("no view");
  return view;
};

describe("CodeEditor", () => {
  it("mounts with the value and an accessible name", () => {
    render(<CodeEditor {...base} />);
    expect(screen.getByLabelText("Body")).toHaveTextContent("hello");
  });

  it("reports user edits through onChange", () => {
    const onChange = vi.fn();
    render(<CodeEditor {...base} onChange={onChange} />);
    act(() => viewOf().dispatch({ changes: { from: 5, insert: "!" } }));
    expect(onChange).toHaveBeenCalledWith("hello!");
  });

  it("applies an external value without echoing onChange and keeps the same view", () => {
    const onChange = vi.fn();
    const { rerender } = render(<CodeEditor {...base} onChange={onChange} />);
    const view = viewOf();
    rerender(<CodeEditor {...base} value="help" onChange={onChange} />);
    expect(viewOf()).toBe(view);
    expect(view.state.doc.toString()).toBe("help");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("is read-only when asked", () => {
    render(<CodeEditor {...base} readOnly />);
    expect(viewOf().state.readOnly).toBe(true);
  });

  it("drops newlines in single-line mode", () => {
    render(<CodeEditor {...base} singleLine />);
    act(() => viewOf().dispatch({ changes: { from: 5, insert: "\nx" } }));
    expect(viewOf().state.doc.toString()).toBe("hellox");
  });

  it("decorates variable tokens", () => {
    render(
      <CodeEditor
        {...base}
        value="{{a}} {{b}}"
        variables={[{ name: "a", value: "1", source: "global", secret: false, resolved: true }]}
      />,
    );
    expect(document.querySelectorAll(".var-ok")).toHaveLength(1);
    expect(document.querySelectorAll(".var-missing")).toHaveLength(1);
  });

  it("draws search marks", () => {
    render(<CodeEditor {...base} marks={{ ranges: [{ from: 0, to: 2 }], current: 0 }} />);
    expect(document.querySelectorAll(".search-mark-current")).toHaveLength(1);
  });

  it("destroys the view on unmount", () => {
    const { unmount } = render(<CodeEditor {...base} />);
    unmount();
    expect(document.querySelector(".cm-editor")).toBeNull();
  });
});
