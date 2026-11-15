import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDialogsStore } from "./dialogsStore";
import { ShellDialogs } from "./ShellDialogs";
import { useTabsStore } from "./tabsStore";

vi.mock("../features/network/NetworkDialog", () => ({ NetworkDialog: () => <div>network dialog</div> }));
vi.mock("../features/curl/ImportCurlDialog", () => ({ ImportCurlDialog: () => <div>import dialog</div> }));
vi.mock("../features/postman/ImportPostmanDialog", () => ({ ImportPostmanDialog: () => <div>postman import dialog</div> }));
vi.mock("../features/postman/ExportPostmanDialog", () => ({
  ExportPostmanDialog: ({ collectionId }: { collectionId: string | null }) => <div>postman export {collectionId}</div>,
}));
vi.mock("../features/cookies/CookieManager", () => ({ CookieManager: () => <div>cookie dialog</div> }));

beforeEach(() => {
  useDialogsStore.setState({ open: null });
  useTabsStore.setState(useTabsStore.getInitialState(), true);
});

describe("ShellDialogs", () => {
  it("renders nothing when closed", () => {
    const { container } = render(<ShellDialogs />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the network and import dialogs", () => {
    useDialogsStore.setState({ open: "network" });
    const { rerender } = render(<ShellDialogs />);
    expect(screen.getByText("network dialog")).toBeInTheDocument();
    useDialogsStore.setState({ open: "import-curl" });
    rerender(<ShellDialogs />);
    expect(screen.getByText("import dialog")).toBeInTheDocument();
  });

  it("renders the Postman dialogs and passes the collection to export", () => {
    useDialogsStore.getState().show("import-postman");
    const { rerender } = render(<ShellDialogs />);
    expect(screen.getByText("postman import dialog")).toBeInTheDocument();
    useDialogsStore.getState().show("export-postman", "c9");
    rerender(<ShellDialogs />);
    expect(screen.getByText("postman export c9")).toBeInTheDocument();
    useDialogsStore.getState().close();
    expect(useDialogsStore.getState().collectionId).toBeNull();
  });

  it("shows cookies only with an active request", () => {
    useDialogsStore.setState({ open: "cookies" });
    const { container } = render(<ShellDialogs />);
    expect(container).toBeEmptyDOMElement();
    expect(useDialogsStore.getState().open).toBeNull();
    useTabsStore.getState().openRequest({ scope: "scratch", id: "s" }, "A");
    expect(container).toBeEmptyDOMElement();
  });

  it("shows cookies for the active request", () => {
    useTabsStore.getState().openRequest({ scope: "scratch", id: "s" }, "A");
    useDialogsStore.setState({ open: "cookies" });
    render(<ShellDialogs />);
    expect(screen.getByText("cookie dialog")).toBeInTheDocument();
  });
});
