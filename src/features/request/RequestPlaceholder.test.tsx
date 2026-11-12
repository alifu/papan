import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useToastStore } from "../../ui/toastStore";
import * as api from "../collections/api";
import { collectionLocation, scratchLocation } from "../locations";
import { RequestPlaceholder } from "./RequestPlaceholder";

vi.mock("../collections/api");

const loc = collectionLocation("c1", "a.http.yaml");
let emit: (id: string) => void = () => {};
const unlisten = vi.fn();

describe("RequestPlaceholder", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useToastStore.setState({ toasts: [] });
    vi.mocked(api.readRequest).mockResolvedValue({ name: "Login", method: "POST", url: "https://x" });
    vi.mocked(api.subscribeCollectionChanged).mockImplementation(async (cb) => {
      emit = cb;
      return unlisten;
    });
  });

  it("loads values and keeps Save disabled until edited", async () => {
    render(<RequestPlaceholder location={loc} />);

    expect(await screen.findByLabelText("URL")).toHaveValue("https://x");
    expect(screen.getByLabelText("Method")).toHaveValue("POST");
    expect(screen.getByRole("heading", { name: "Login" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("saves { method, url } and disables Save again", async () => {
    vi.mocked(api.writeRequest).mockResolvedValue(null);
    render(<RequestPlaceholder location={loc} />);
    await userEvent.type(await screen.findByLabelText("URL"), "/v1");
    await userEvent.selectOptions(screen.getByLabelText("Method"), "PUT");

    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(api.writeRequest).toHaveBeenCalledWith(loc, { method: "PUT", url: "https://x/v1" });
    await vi.waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeDisabled());
  });

  it("returning to the saved values clears the edit", async () => {
    render(<RequestPlaceholder location={loc} />);
    await userEvent.type(await screen.findByLabelText("URL"), "a");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
    await userEvent.type(screen.getByLabelText("URL"), "{Backspace}");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("a save failure toasts and keeps the edits", async () => {
    vi.mocked(api.writeRequest).mockRejectedValue({ code: "io", message: "Read-only" });
    render(<RequestPlaceholder location={loc} />);
    await userEvent.type(await screen.findByLabelText("URL"), "z");

    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await vi.waitFor(() => expect(useToastStore.getState().toasts[0]?.message).toBe("Read-only"));
    expect(screen.getByLabelText("URL")).toHaveValue("https://xz");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("reloads on a change event when unedited", async () => {
    render(<RequestPlaceholder location={loc} />);
    await screen.findByLabelText("URL");
    vi.mocked(api.readRequest).mockResolvedValue({ name: "Login", method: "POST", url: "https://new" });

    act(() => emit("c1"));

    await vi.waitFor(() => expect(screen.getByLabelText("URL")).toHaveValue("https://new"));
  });

  it("keeps edits on a change event and ignores other collections", async () => {
    render(<RequestPlaceholder location={loc} />);
    await userEvent.type(await screen.findByLabelText("URL"), "!");
    vi.mocked(api.readRequest).mockClear();

    act(() => emit("other"));
    expect(api.readRequest).not.toHaveBeenCalled();
    act(() => emit("c1"));

    await vi.waitFor(() => expect(api.readRequest).toHaveBeenCalledTimes(1));
    expect(screen.getByLabelText("URL")).toHaveValue("https://x!");
  });

  it("shows a message when the request is gone", async () => {
    vi.mocked(api.readRequest).mockRejectedValue({ code: "not_found", message: "x" });
    render(<RequestPlaceholder location={loc} />);
    expect(await screen.findByText("This request no longer exists on disk.")).toBeVisible();
  });

  it("toasts other load errors and renders nothing", async () => {
    vi.mocked(api.readRequest).mockRejectedValue({ code: "io", message: "Unreadable" });
    const { container } = render(<RequestPlaceholder location={scratchLocation("s")} />);
    await vi.waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(container).toBeEmptyDOMElement();
    expect(api.subscribeCollectionChanged).not.toHaveBeenCalled();
  });

  it("offers a custom verb from the file and unsubscribes on unmount", async () => {
    vi.mocked(api.readRequest).mockResolvedValue({ name: "L", method: "PURGE", url: "" });
    const { unmount } = render(<RequestPlaceholder location={loc} />);
    expect(await screen.findByRole("option", { name: "PURGE" })).toBeInTheDocument();
    unmount();
    expect(unlisten).toHaveBeenCalled();
  });

  it("unsubscribes even if unmounted before the subscription resolves", async () => {
    const { unmount } = render(<RequestPlaceholder location={loc} />);
    unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalled());
  });
});
