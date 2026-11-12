import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as collections from "../collections/api";
import { scratchLocation } from "../locations";
import * as api from "./api";
import { emptyContent } from "./content";
import { RequestEditor } from "./RequestEditor";
import { useRequestStore } from "./requestStore";

vi.mock("../collections/api");
vi.mock("./api");
vi.mock("../response/ResponseViewer", () => ({ ResponseViewer: () => <div>viewer</div> }));

const loc = scratchLocation("s1");
const content = { ...emptyContent(), url: "http://x/json?a=1" };

const open = async () => {
  render(<RequestEditor tabId="t" location={loc} />);
  return screen.findByLabelText("URL");
};

describe("RequestEditor", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useRequestStore.setState({ byTab: {} });
    vi.mocked(collections.readRequest).mockResolvedValue({ name: "R", content });
  });

  it("keeps the URL and params table in sync both ways", async () => {
    const url = await open();
    expect(screen.getByLabelText("Name 1")).toHaveValue("a");

    await userEvent.type(screen.getByLabelText("Name 2"), "b");
    await userEvent.type(screen.getByLabelText("Value 2"), "2");
    expect(url).toHaveValue("http://x/json?a=1&b=2");

    await userEvent.click(screen.getByLabelText("Enable row 1"));
    expect(url).toHaveValue("http://x/json?b=2");

    await userEvent.type(url, "&c=3");
    expect(screen.getByLabelText("Name 3")).toHaveValue("c");
  });

  it("enables Save only when dirty and shows Cancel while sending", async () => {
    const url = await open();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await userEvent.type(url, "z");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();

    vi.mocked(api.sendRequest).mockReturnValue(new Promise(() => {}));
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();
  });

  it("shows the TLS badge only when verification is off", async () => {
    await open();
    expect(screen.queryByText("TLS not verified")).toBeNull();
    await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
    await userEvent.click(screen.getByLabelText("Verify TLS certificates"));
    expect(screen.getByText("TLS not verified")).toBeVisible();
  });

  it("keeps text when switching JSON to XML and formats valid JSON", async () => {
    await open();
    await userEvent.click(screen.getByRole("tab", { name: "Body" }));
    await userEvent.selectOptions(screen.getByLabelText("Body type"), "json");
    await userEvent.click(screen.getByLabelText("Body text"));
    await userEvent.paste('{"a":1}');
    await userEvent.click(screen.getByRole("button", { name: "Format" }));
    expect(screen.getByLabelText("Body text")).toHaveValue('{\n  "a": 1\n}');
    await userEvent.selectOptions(screen.getByLabelText("Body type"), "xml");
    expect(screen.getByLabelText("Body text")).toHaveValue('{\n  "a": 1\n}');
  });

  it("choose file stores the picked path; cancel changes nothing", async () => {
    await open();
    await userEvent.click(screen.getByRole("tab", { name: "Body" }));
    await userEvent.selectOptions(screen.getByLabelText("Body type"), "binary");
    vi.mocked(api.pickBodyFile).mockResolvedValueOnce(null);
    await userEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    expect(screen.getByText("No file chosen")).toBeVisible();

    vi.mocked(api.pickBodyFile).mockResolvedValueOnce({ path: "/a/b.bin", name: "b.bin", size_bytes: 1 });
    await userEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    expect(await screen.findByText("b.bin")).toBeVisible();
    expect(screen.getByText("b.bin")).toHaveAttribute("title", "/a/b.bin");
    // Grants are scoped to the request's location, so the picker must receive it.
    expect(api.pickBodyFile).toHaveBeenLastCalledWith(expect.objectContaining({ scope: expect.any(String) }));
  });

  it("shows auth fields per type with show/hide and the hint, and validates timeout", async () => {
    await open();
    await userEvent.click(screen.getByRole("tab", { name: "Auth" }));
    await userEvent.selectOptions(screen.getByLabelText("Auth type"), "bearer");
    expect(screen.getByLabelText("Token")).toHaveAttribute("type", "password");
    await userEvent.click(screen.getByLabelText("Show token"));
    expect(screen.getByLabelText("Token")).toHaveAttribute("type", "text");
    expect(screen.getByText(/Kept until you quit/)).toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "Settings" }));
    await userEvent.type(screen.getByLabelText(/Timeout/), "99");
    expect(screen.getByLabelText(/Timeout/)).toHaveValue("3000099");
    expect(screen.getByRole("alert")).toBeVisible();
  });

  it("renders unsupported sections read-only", async () => {
    vi.mocked(collections.readRequest).mockResolvedValue({
      name: "R",
      content: { ...content, auth: { type: "unsupported" }, body: { type: "unsupported" } },
    });
    await open();
    await userEvent.click(screen.getByRole("tab", { name: "Auth" }));
    expect(screen.getByText(/Made by a newer Papan/)).toBeVisible();
    await userEvent.click(screen.getByRole("tab", { name: "Body" }));
    expect(screen.getByText(/Made by a newer Papan/)).toBeVisible();
  });
});
