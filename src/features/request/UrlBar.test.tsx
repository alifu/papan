import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as envApi from "../environments/api";
import userEvent from "@testing-library/user-event";
import { UrlBar, isCurlText } from "./UrlBar";

vi.mock("../environments/api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const base = {
  method: "GET",
  url: "{{host}}/x",
  location: { scope: "scratch" as const, id: "s" },
  variables: [],
  sending: false,
  dirty: false,
  tlsVerified: true,
  onMethod: vi.fn(),
  onUrl: vi.fn(),
  onSend: vi.fn(),
  onCancel: vi.fn(),
  onSave: vi.fn(),
};

beforeEach(() => vi.resetAllMocks());

describe("UrlBar", () => {
  it("shows the resolved URL as a tooltip once the debounce passes", async () => {
    vi.mocked(envApi.previewTemplate).mockResolvedValue({ text: "https://h/x", unresolved: [] });
    render(<UrlBar {...base} />);
    await vi.waitFor(() => expect(screen.getByLabelText("URL").parentElement).toHaveAttribute("title", "https://h/x"));
  });

  it("does not ask for a preview when the URL has no variables", () => {
    render(<UrlBar {...base} url="https://plain" />);
    expect(envApi.previewTemplate).not.toHaveBeenCalled();
  });

  it("shows the Proxy badge only when a proxy is active", () => {
    const { rerender } = render(<UrlBar {...base} url="https://p" />);
    expect(screen.queryByText("Proxy")).toBeNull();
    rerender(<UrlBar {...base} url="https://p" proxyActive />);
    expect(screen.getByText("Proxy")).toBeInTheDocument();
  });

  it("asks before importing a pasted cURL command", async () => {
    const onImportCurl = vi.fn();
    render(<UrlBar {...base} url="https://p" onImportCurl={onImportCurl} />);
    fireEvent.paste(screen.getByLabelText("URL"), { clipboardData: { getData: () => "curl -X POST https://x" } });
    expect(screen.getByText("Replace this request with the pasted cURL command?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Replace" }));
    expect(onImportCurl).toHaveBeenCalledWith("curl -X POST https://x");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("leaves other pastes alone and can decline the import", async () => {
    const onImportCurl = vi.fn();
    render(<UrlBar {...base} url="https://p" onImportCurl={onImportCurl} />);
    fireEvent.paste(screen.getByLabelText("URL"), { clipboardData: { getData: () => "https://other" } });
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.paste(screen.getByLabelText("URL"), { clipboardData: { getData: () => "curl https://x" } });
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onImportCurl).not.toHaveBeenCalled();
  });

  it("recognises cURL text", () => {
    expect(isCurlText("  curl https://x")).toBe(true);
    expect(isCurlText("curling")).toBe(false);
  });
});
