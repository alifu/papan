import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PostmanFileSummary, PostmanPreview } from "../../ipc/bindings";
import { makeTree } from "../../test/fixtures";
import { useToastStore } from "../../ui/toastStore";
import { useCollectionsStore } from "../collections/collectionsStore";
import * as collectionsApi from "../collections/api";
import * as envApi from "../environments/api";
import * as api from "./api";
import { ImportPostmanDialog } from "./ImportPostmanDialog";

vi.mock("./api");
vi.mock("../collections/api");
vi.mock("../environments/api");

const summary = (over: Partial<PostmanFileSummary> = {}): PostmanFileSummary => ({
  file: "api.json",
  kind: "collection_v21",
  name: "Shop",
  requests: 3,
  folders: 1,
  variables: 2,
  scripts: 2,
  secret_candidates: ["token", "apiKey"],
  ...over,
});
const previewOf = (files: PostmanFileSummary[] = [summary()]): PostmanPreview => ({
  token: "tok",
  files,
  report: [{ level: "warning", file: "api.json", path: "$.item[0]", message: "pm.cookies is not supported" }],
  truncated: 0,
});

async function choose(preview: PostmanPreview) {
  vi.mocked(api.importPick).mockResolvedValue(preview);
  await userEvent.click(screen.getByRole("button", { name: "Choose files…" }));
  await screen.findByRole("list", { name: "Files" });
}

describe("ImportPostmanDialog", () => {
  const onClose = vi.fn();
  beforeEach(() => {
    vi.resetAllMocks();
    useCollectionsStore.setState(useCollectionsStore.getInitialState(), true);
    useCollectionsStore.setState({ trees: [makeTree()] });
    useToastStore.setState({ toasts: [] });
    vi.mocked(api.importDiscard).mockResolvedValue(null);
    vi.mocked(envApi.listEnvironments).mockResolvedValue({ environments: [], selected: null });
    vi.mocked(collectionsApi.listCollections).mockResolvedValue([makeTree(), makeTree("c2", "Shop")]);
  });

  it("stays on step 1 when the file dialog is cancelled, without an error", async () => {
    vi.mocked(api.importPick).mockResolvedValue(null);
    render(<ImportPostmanDialog onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Choose files…" }));
    expect(screen.getByRole("button", { name: "Choose files…" })).toBeEnabled();
    expect(useToastStore.getState().toasts).toEqual([]);
  });

  it("shows counts, defaults, scripts warning and report", async () => {
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf());
    expect(screen.getByText(/3 requests, 1 folders, 2\s+variables, 2 scripts/)).toBeVisible();
    expect(screen.getByLabelText("token")).toBeChecked();
    expect(screen.getByLabelText("apiKey")).toBeChecked();
    expect(screen.getByLabelText("Store secret values on this machine")).not.toBeChecked();
    expect(screen.getByLabelText("Import scripts")).toBeChecked();
    expect(screen.getByText(/switched off/)).toBeVisible();
    expect(screen.getByText("pm.cookies is not supported")).toBeVisible();
  });

  it("commits the user's choices, refreshes stores and shows the result report", async () => {
    vi.mocked(api.importCommit).mockResolvedValue({
      collection_id: "c2",
      environments: ["dev"],
      globals: 0,
      report: [{ level: "info", file: "api.json", path: "", message: "3 scripts imported but switched off" }],
    });
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf());
    await userEvent.click(screen.getByLabelText("apiKey"));
    await userEvent.click(screen.getByRole("button", { name: "Import" }));

    expect(api.importCommit).toHaveBeenCalledWith("tok", {
      target: { type: "new_collection" },
      secret_names: ["token"],
      store_secret_values: false,
      import_scripts: true,
      globals_clash: "keep",
    });
    expect(await screen.findByText(/3 scripts imported but switched off/)).toBeVisible();
    expect(collectionsApi.listCollections).toHaveBeenCalled();
    expect(useCollectionsStore.getState().expanded.has("c2:")).toBe(true);
    expect(envApi.listEnvironments).toHaveBeenCalledWith("c2");
    expect(api.importDiscard).not.toHaveBeenCalled();
  });

  it("still shows the report when refreshing the stores fails after a successful import", async () => {
    vi.mocked(api.importCommit).mockResolvedValue({
      collection_id: "c2",
      environments: [],
      globals: 0,
      report: [{ level: "info", file: "api.json", path: "", message: "imported fine" }],
    });
    vi.mocked(collectionsApi.listCollections).mockRejectedValue(new Error("refresh failed"));
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf());
    await userEvent.click(screen.getByRole("button", { name: "Import" }));

    expect(await screen.findByText(/imported fine/)).toBeVisible();
    expect(useToastStore.getState().toasts.length).toBeGreaterThan(0);
  });

  it("keeps the preview when the folder dialog is cancelled on commit", async () => {
    vi.mocked(api.importCommit).mockResolvedValue(null);
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf());
    await userEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(screen.getByRole("list", { name: "Files" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Import" })).toBeEnabled();
    expect(useToastStore.getState().toasts).toEqual([]);
  });

  it("environment-only preview forces an open collection", async () => {
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf([summary({ kind: "environment", secret_candidates: [] })]));
    expect(screen.getByRole("option", { name: "New collection" })).toBeDisabled();
    expect(screen.getByLabelText("Import into")).toHaveValue("collection");
    expect(screen.getByLabelText("Collection")).toHaveValue("c1");
  });

  it("environment-only with no open collection blocks Import", async () => {
    useCollectionsStore.setState({ trees: [] });
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf([summary({ kind: "environment", secret_candidates: [] })]));
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(/Open a collection/);
  });

  it("offers the globals clash choice only with a globals file", async () => {
    vi.mocked(api.importCommit).mockResolvedValue({ collection_id: null, environments: [], globals: 2, report: [] });
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf([summary({ kind: "globals", secret_candidates: [] })]));
    await userEvent.selectOptions(screen.getByLabelText("Existing globals"), "overwrite");
    await userEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(vi.mocked(api.importCommit).mock.calls[0][1]).toMatchObject({
      target: { type: "collection", collection_id: "c1" },
      globals_clash: "overwrite",
    });
  });

  it("discards the stage when cancelled at the preview", async () => {
    render(<ImportPostmanDialog onClose={onClose} />);
    await choose(previewOf());
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.importDiscard).toHaveBeenCalledWith("tok");
    expect(onClose).toHaveBeenCalled();
  });

  it("does not discard anything when cancelled before picking", async () => {
    render(<ImportPostmanDialog onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.importDiscard).not.toHaveBeenCalled();
  });

  it("discards a preview that arrives after the dialog closed", async () => {
    let resolve: (p: PostmanPreview) => void = () => undefined;
    vi.mocked(api.importPick).mockReturnValue(new Promise((r) => (resolve = r)));
    const { unmount } = render(<ImportPostmanDialog onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Choose files…" }));
    unmount();
    resolve(previewOf());
    await vi.waitFor(() => expect(api.importDiscard).toHaveBeenCalledWith("tok"));
  });

  it("reports a failed pick as a toast", async () => {
    vi.mocked(api.importPick).mockRejectedValue({ code: "invalid_input", message: "Not a Postman file." });
    render(<ImportPostmanDialog onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Choose files…" }));
    await vi.waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
  });
});
