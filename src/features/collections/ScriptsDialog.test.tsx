import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { ScriptsDialog } from "./ScriptsDialog";

vi.mock("./api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const target = { scope: "folder", collection_id: "c1", path: "auth" } as const;

describe("ScriptsDialog", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.readScripts).mockResolvedValue({ enabled: false, pre_request: "a();", post_response: "" });
    vi.mocked(api.writeScripts).mockResolvedValue(null);
  });

  it("loads, edits and saves the scripts", async () => {
    const onClose = vi.fn();
    render(<ScriptsDialog title="Folder scripts" target={target} onClose={onClose} />);
    const editor = await screen.findByLabelText("Pre-request script");
    expect(editor).toHaveValue("a();");
    expect(screen.getByRole("note")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "Enabled" }));
    await userEvent.type(editor, "b");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.readScripts).toHaveBeenCalledWith(target);
    expect(api.writeScripts).toHaveBeenCalledWith(target, { enabled: true, pre_request: "a();b", post_response: "" });
    expect(onClose).toHaveBeenCalled();
  });

  it("stays open when saving fails", async () => {
    vi.mocked(api.writeScripts).mockRejectedValue({ code: "internal", message: "x", detail: null });
    const onClose = vi.fn();
    render(<ScriptsDialog title="Folder scripts" target={target} onClose={onClose} />);
    await screen.findByLabelText("Pre-request script");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onClose).not.toHaveBeenCalled();
  });
});
