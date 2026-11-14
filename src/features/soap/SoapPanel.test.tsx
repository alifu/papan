import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RequestContent } from "../../ipc/bindings";
import { useToastStore } from "../../ui/toastStore";
import { emptyContent } from "../request/content";
import * as api from "./api";
import { makeTemplate, makeWsdl } from "./fixtures";
import SoapPanel from "./SoapPanel";

vi.mock("./api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const location = { scope: "scratch" as const, id: "s1" };
const soapContent = (over: Partial<RequestContent> = {}): RequestContent => ({
  ...emptyContent(),
  kind: "soap",
  method: "POST",
  body: { type: "xml", text: "" },
  ...over,
});

/** Applies the panel's pure updates to a live copy, like the request store does. */
function setup(initial: RequestContent) {
  let current = initial;
  const onChange = vi.fn((update: (c: RequestContent) => RequestContent) => {
    current = update(current);
    view.rerender(<SoapPanel location={location} content={current} onChange={onChange} />);
  });
  const view = render(<SoapPanel location={location} content={current} onChange={onChange} />);
  return { get: () => current, onChange };
}

async function importWsdlViaDialog(wsdl = makeWsdl()) {
  vi.mocked(api.importWsdl).mockResolvedValue(wsdl);
  await userEvent.click(screen.getByRole("button", { name: "Import WSDL…" }));
  await userEvent.type(screen.getByLabelText("WSDL URL"), "http://h/svc?wsdl{Enter}");
  await userEvent.click(await screen.findByRole("button", { name: "Use this WSDL" }));
}

beforeEach(() => {
  vi.resetAllMocks();
  useToastStore.setState({ toasts: [] });
});

describe("SoapPanel", () => {
  it("edits the version and SOAPAction", async () => {
    const view = setup(soapContent());
    await userEvent.selectOptions(screen.getByLabelText("SOAP version"), "1.2");
    await userEvent.type(screen.getByLabelText("SOAPAction"), "u");
    expect(view.get().soap).toMatchObject({ version: "1.2", action: "u" });
  });

  it("generates an envelope from a picked operation and fills body, soap and URL", async () => {
    vi.mocked(api.soapEnvelope).mockResolvedValue(makeTemplate());
    const view = setup(soapContent());
    await importWsdlViaDialog();

    await userEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(api.soapEnvelope).toHaveBeenCalledWith("w1", "Calc", "CalcSoap12", "Add", { include_optional: true });
    expect(view.get().body).toEqual({ type: "xml", text: "<soap:Envelope/>" });
    expect(view.get().url).toBe("http://h/calc");
    expect(view.get().soap).toEqual({
      version: "1.2",
      action: "urn:Add",
      wsdl: { url: "http://h/svc?wsdl", file: "", service: "Calc", port: "CalcSoap12", operation: "Add" },
    });
  });

  it.each([
    ["a URL with a variable", makeWsdl({ source: "{{base}}/svc?wsdl", ref_url: "{{base}}/svc?wsdl" }), { url: "{{base}}/svc?wsdl", file: "" }],
    ["a picked file", makeWsdl({ source: "calc.wsdl", ref_url: "" }), { url: "", file: "calc.wsdl" }],
  ])("saves the backend's reference for %s", async (_name, wsdl, saved) => {
    vi.mocked(api.soapEnvelope).mockResolvedValue(makeTemplate());
    const view = setup(soapContent());
    await importWsdlViaDialog(wsdl);

    await userEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(view.get().soap.wsdl).toEqual({ ...saved, service: "Calc", port: "CalcSoap12", operation: "Add" });
  });

  it("passes the optional-elements switch to the generator", async () => {
    vi.mocked(api.soapEnvelope).mockResolvedValue(makeTemplate());
    setup(soapContent());
    await importWsdlViaDialog();
    await userEvent.click(screen.getByRole("checkbox", { name: "Include optional elements" }));
    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(api.soapEnvelope).toHaveBeenCalledWith("w1", "Calc", "CalcSoap12", "Add", { include_optional: false });
  });

  it("asks before replacing a typed envelope and keeps it on cancel", async () => {
    vi.mocked(api.soapEnvelope).mockResolvedValue(makeTemplate());
    const view = setup(soapContent({ body: { type: "xml", text: "<mine/>" }, url: "http://mine" }));
    await importWsdlViaDialog();

    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByRole("dialog", { name: "Replace envelope?" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(view.get().body).toEqual({ type: "xml", text: "<mine/>" });

    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    await userEvent.click(await screen.findByRole("button", { name: "Replace" }));
    expect(view.get().body).toEqual({ type: "xml", text: "<soap:Envelope/>" });
    expect(view.get().url).toBe("http://h/calc");
  });

  it("restores the operation list of a saved request from the cache", async () => {
    vi.mocked(api.readCachedWsdl).mockResolvedValue(makeWsdl());
    const wsdl = { url: "http://h/svc?wsdl", file: "", service: "Calc", port: "CalcSoap12", operation: "Add" };
    setup(soapContent({ soap: { version: "1.2", action: "urn:Add", wsdl } }));

    expect(await screen.findByRole("button", { name: "Add" })).toHaveAttribute("aria-pressed", "true");
    expect(api.readCachedWsdl).toHaveBeenCalledWith(location, wsdl);
    expect(screen.getByRole("button", { name: "Regenerate envelope" })).toBeEnabled();
  });

  it("explains when a saved WSDL is no longer cached", async () => {
    vi.mocked(api.readCachedWsdl).mockResolvedValue(null);
    const wsdl = { url: "", file: "calc.wsdl", service: "", port: "", operation: "" };
    setup(soapContent({ soap: { version: "1.1", action: "", wsdl } }));
    expect(await screen.findByText(/not cached/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Regenerate envelope" })).toBeDisabled();
  });

  it("reports an envelope failure as a toast and leaves the request alone", async () => {
    vi.mocked(api.soapEnvelope).mockRejectedValue({ code: "internal", message: "boom" });
    const view = setup(soapContent());
    await importWsdlViaDialog();
    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    await vi.waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(view.get().body).toEqual({ type: "xml", text: "" });
  });
});
