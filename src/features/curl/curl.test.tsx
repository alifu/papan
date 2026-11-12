import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CurlImport } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { useToastStore } from "../../ui/toastStore";
import * as collectionsApi from "../collections/api";
import { emptyContent } from "../request/content";
import { useRequestStore } from "../request/requestStore";
import { useScratchStore } from "../scratch/scratchStore";
import * as api from "./api";
import { COPIED_MESSAGE, copyAsCurl } from "./copyAsCurl";
import { ImportCurlDialog } from "./ImportCurlDialog";

vi.mock("./api");
vi.mock("../collections/api");
vi.mock("../response/clipboard", () => ({ copyText: vi.fn().mockResolvedValue(undefined) }));

const location = { scope: "scratch" as const, id: "s1" };
const imported: CurlImport = { content: { ...emptyContent(), url: "https://x" }, warnings: ["Ignored -x"] };

beforeEach(() => {
  vi.resetAllMocks();
  useToastStore.setState({ toasts: [] });
  useTabsStore.setState(useTabsStore.getInitialState(), true);
  useRequestStore.setState({ byTab: {} });
});

describe("copyAsCurl", () => {
  it("copies the export of the active request and toasts", async () => {
    const id = useTabsStore.getState().openRequest(location, "A");
    useRequestStore.setState({ byTab: { [id]: { doc: { name: "A", content: emptyContent() }, draft: null } as never } });
    vi.mocked(api.exportCurl).mockResolvedValue("curl 'https://x'");
    await copyAsCurl();
    const { copyText } = await import("../response/clipboard");
    expect(copyText).toHaveBeenCalledWith("curl 'https://x'");
    expect(useToastStore.getState().toasts[0]?.message).toBe(COPIED_MESSAGE);
  });

  it("does nothing without an active request", async () => {
    await copyAsCurl();
    expect(api.exportCurl).not.toHaveBeenCalled();
  });

  it("toasts an export error", async () => {
    const id = useTabsStore.getState().openRequest(location, "A");
    useRequestStore.setState({ byTab: { [id]: { doc: { name: "A", content: emptyContent() }, draft: null } as never } });
    vi.mocked(api.exportCurl).mockRejectedValue({ code: "internal", message: "bad" });
    await copyAsCurl();
    expect(useToastStore.getState().toasts[0]?.kind).toBe("error");
  });
});

describe("ImportCurlDialog", () => {
  const stubScratch = () =>
    useScratchStore.setState({ create: vi.fn().mockResolvedValue({ id: "s1", name: "U", method: "GET" }) });

  it("creates a scratch request and lists warnings", async () => {
    stubScratch();
    vi.mocked(api.importCurl).mockResolvedValue(imported);
    vi.mocked(collectionsApi.writeRequest).mockResolvedValue(null);
    render(<ImportCurlDialog onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Create request" })).toBeDisabled();
    await userEvent.type(screen.getByLabelText("cURL command"), "curl -x p https://x");
    await userEvent.click(screen.getByRole("button", { name: "Create request" }));
    expect(await screen.findByText("Ignored -x")).toBeInTheDocument();
    expect(collectionsApi.writeRequest).toHaveBeenCalledWith(location, imported.content);
  });

  it("closes when there are no warnings", async () => {
    stubScratch();
    const onClose = vi.fn();
    vi.mocked(api.importCurl).mockResolvedValue({ ...imported, warnings: [] });
    vi.mocked(collectionsApi.writeRequest).mockResolvedValue(null);
    render(<ImportCurlDialog onClose={onClose} />);
    await userEvent.type(screen.getByLabelText("cURL command"), "curl https://x");
    await userEvent.click(screen.getByRole("button", { name: "Create request" }));
    await vi.waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("toasts a parse error and creates nothing", async () => {
    stubScratch();
    vi.mocked(api.importCurl).mockRejectedValue({ code: "invalid_input", message: "Not a cURL command." });
    render(<ImportCurlDialog onClose={vi.fn()} />);
    await userEvent.type(screen.getByLabelText("cURL command"), "ls");
    await userEvent.click(screen.getByRole("button", { name: "Create request" }));
    await vi.waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(collectionsApi.writeRequest).not.toHaveBeenCalled();
  });
});
