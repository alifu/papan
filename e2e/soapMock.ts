import type { Page } from "@playwright/test";

/**
 * In-memory fake of the M6 SOAP commands (`import_wsdl`, `read_cached_wsdl`, `soap_envelope`) plus the
 * `kind` handling of `create_scratch`, `list_scratch` and `read_request` for scratch requests. A WSDL
 * URL containing `bad` fails like a WSDL 2.0 document. Must run after the base fake has published
 * `window.__handlers` (call it from the end of `mockTauri`, after `installNetFake`).
 */
export async function installSoapFake(page: Page): Promise<void> {
  await page.addInitScript(() => {
    type Args = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
    const w = window as unknown as { __handlers: Record<string, (a: Args) => any> }; // eslint-disable-line @typescript-eslint/no-explicit-any
    const handlers = w.__handlers;
    const appError = (code: string, message: string) => ({ code, message });

    const kinds = new Map<string, string>();
    const original = { create_scratch: handlers.create_scratch, list_scratch: handlers.list_scratch, read_request: handlers.read_request };
    const withKind = (item: { id: string; method: string }) => ({ ...item, kind: kinds.get(item.id) ?? "http" });

    const wsdl = (source: string) => ({
      id: "wsdl-1",
      source,
      services: [{
        name: "CalculatorService",
        ports: [{
          name: "CalculatorSoap12",
          version: "1.2",
          address: "http://localhost/calc",
          operations: [
            { name: "Add", action: "http://tempuri.org/Add", style: "document", documentation: "Adds two numbers" },
            { name: "Subtract", action: "http://tempuri.org/Subtract", style: "document", documentation: "" },
          ],
        }],
      }],
      fetched: [source, "http://localhost/calc.xsd"],
      warnings: ["Skipped the HTTP binding CalculatorHttpGet."],
    });
    const envelope = (operation: string, optional: boolean) =>
      [
        '<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope" xmlns:ns1="http://tempuri.org/">',
        "  <soap:Header/>",
        "  <soap:Body>",
        `    <ns1:${operation}>`,
        "      <ns1:a>?</ns1:a>",
        optional ? "      <!-- Optional: -->\n      <ns1:note>?</ns1:note>" : "",
        `    </ns1:${operation}>`,
        "  </soap:Body>",
        "</soap:Envelope>",
      ].filter(Boolean).join("\n");

    Object.assign(handlers, {
      create_scratch: (a: Args) => {
        const item = original.create_scratch(a) as { id: string; name: string; method: string };
        if (a.kind && a.kind !== "http") kinds.set(item.id, String(a.kind));
        return withKind(item);
      },
      list_scratch: () => (original.list_scratch({}) as { id: string; method: string }[]).map(withKind),
      read_request: (a: Args) => {
        const doc = original.read_request(a) as { name: string; content: Record<string, unknown> };
        const kind = a.location.scope === "scratch" ? kinds.get(a.location.id) : undefined;
        if (!kind || doc.content.kind) return doc;
        const body = kind === "soap" ? { type: "xml", text: "" } : doc.content.body;
        return { ...doc, content: { ...doc.content, kind, method: "POST", body, soap: { version: "1.1", action: "", wsdl: null } } };
      },
      import_wsdl: (a: Args) => {
        if (a.source.type === "pick_file") return wsdl("calculator.wsdl");
        if (String(a.source.url).includes("bad")) throw appError("invalid_input", "WSDL 2.0 is not supported");
        return wsdl(String(a.source.url));
      },
      read_cached_wsdl: (a: Args) => wsdl(a.wsdl.url || a.wsdl.file),
      soap_envelope: (a: Args) => ({
        version: "1.2",
        action: a.operation === "Add" ? "http://tempuri.org/Add" : "http://tempuri.org/Subtract",
        address: "http://localhost/calc",
        envelope: envelope(String(a.operation), Boolean(a.options.include_optional)),
      }),
    });
  });
}
