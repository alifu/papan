import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { makeWsdl } from "./fixtures";
import { WsdlImportDialog } from "./WsdlImportDialog";

vi.mock("./api");

const location = { scope: "scratch" as const, id: "s1" };
const setup = () => {
  const onImported = vi.fn();
  const onClose = vi.fn();
  render(<WsdlImportDialog location={location} onImported={onImported} onClose={onClose} />);
  return { onImported, onClose };
};

beforeEach(() => vi.resetAllMocks());

describe("WsdlImportDialog", () => {
  it("imports from a URL, lists fetched documents and warnings, and hands the result over", async () => {
    vi.mocked(api.importWsdl).mockResolvedValue(makeWsdl());
    const { onImported, onClose } = setup();

    await userEvent.type(screen.getByLabelText("WSDL URL"), "http://h/svc?wsdl");
    await userEvent.click(screen.getByRole("button", { name: "Import" }));

    expect(await screen.findByText("Skipped HTTP binding.")).toBeVisible();
    expect(screen.getByLabelText("Fetched documents")).toHaveTextContent("http://h/types.xsd");
    expect(api.importWsdl).toHaveBeenCalledWith(expect.any(String), location, {
      type: "url",
      url: "http://h/svc?wsdl",
    });

    await userEvent.click(screen.getByRole("button", { name: "Use this WSDL" }));
    expect(onImported).toHaveBeenCalledWith(makeWsdl());
    expect(onClose).toHaveBeenCalled();
  });

  it("imports from a file through the native dialog", async () => {
    vi.mocked(api.importWsdl).mockResolvedValue(makeWsdl({ source: "calc.wsdl" }));
    setup();
    await userEvent.click(screen.getByRole("button", { name: "From file…" }));
    await screen.findByText(/calc\.wsdl/);
    expect(api.importWsdl).toHaveBeenCalledWith(expect.any(String), location, { type: "pick_file" });
  });

  it("stays empty when the file dialog is cancelled", async () => {
    vi.mocked(api.importWsdl).mockResolvedValue(null);
    setup();
    await userEvent.click(screen.getByRole("button", { name: "From file…" }));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(screen.queryByRole("button", { name: "Use this WSDL" })).toBeNull();
  });

  it("shows the backend message on failure", async () => {
    vi.mocked(api.importWsdl).mockRejectedValue({ code: "invalid_input", message: "WSDL 2.0 is not supported" });
    setup();
    await userEvent.type(screen.getByLabelText("WSDL URL"), "http://h/x{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("WSDL 2.0 is not supported");
  });

  it("requires a URL", async () => {
    setup();
    await userEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(screen.getByRole("alert")).toBeVisible();
    expect(api.importWsdl).not.toHaveBeenCalled();
  });

  it("cancels a running import without showing an error", async () => {
    let reject: (e: unknown) => void = () => {};
    vi.mocked(api.importWsdl).mockReturnValue(new Promise((_, r) => (reject = r)));
    vi.mocked(api.cancelImport).mockResolvedValue(null);
    setup();
    await userEvent.type(screen.getByLabelText("WSDL URL"), "http://h/x{Enter}");

    await userEvent.click(await screen.findByRole("button", { name: "Stop import" }));
    expect(api.cancelImport).toHaveBeenCalledWith(vi.mocked(api.importWsdl).mock.calls[0]![0]);
    reject({ code: "cancelled", message: "Cancelled" });

    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
