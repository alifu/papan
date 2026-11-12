import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as envApi from "../environments/api";
import { UrlBar } from "./UrlBar";

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
});
