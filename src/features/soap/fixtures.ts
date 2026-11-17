import type { SoapTemplate, WsdlImport } from "../../ipc/bindings";

export const makeWsdl = (over: Partial<WsdlImport> = {}): WsdlImport => ({
  id: "w1",
  source: "http://h/svc?wsdl",
  ref_url: "http://h/svc?wsdl",
  services: [
    {
      name: "Calc",
      ports: [
        {
          name: "CalcSoap12",
          version: "1.2",
          address: "http://h/calc",
          operations: [
            { name: "Add", action: "urn:Add", style: "document", documentation: "Adds" },
            { name: "Sub", action: "urn:Sub", style: "document", documentation: "" },
          ],
        },
      ],
    },
  ],
  fetched: ["http://h/svc?wsdl", "http://h/types.xsd"],
  warnings: ["Skipped HTTP binding."],
  ...over,
});

export const makeTemplate = (over: Partial<SoapTemplate> = {}): SoapTemplate => ({
  version: "1.2",
  action: "urn:Add",
  address: "http://h/calc",
  envelope: "<soap:Envelope/>",
  ...over,
});
