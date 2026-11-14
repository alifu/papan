import type { RequestContent, SoapTemplate, WsdlRef } from "../../ipc/bindings";

/** True when applying `template` would overwrite something the user may have typed. */
export function needsConfirmation(content: RequestContent, template: SoapTemplate): boolean {
  const { body, url } = content;
  const bodyIsMine =
    body.type === "none" ||
    (body.type === "xml" && (body.text.trim() === "" || body.text === template.envelope));
  const urlIsMine = url === "" || template.address === "" || url === template.address;
  return !bodyIsMine || !urlIsMine;
}

/**
 * Writes the generated envelope to the XML body, `action`/`version` and the WSDL reference to `soap`,
 * and the address to the URL only when the URL is empty or `replaceUrl` is set. Returns a new value.
 */
export function applyTemplate(
  content: RequestContent,
  template: SoapTemplate,
  ref: WsdlRef,
  replaceUrl: boolean,
): RequestContent {
  const setUrl = template.address !== "" && (content.url === "" || replaceUrl);
  return {
    ...content,
    url: setUrl ? template.address : content.url,
    body: { type: "xml", text: template.envelope },
    soap: { version: template.version, action: template.action, wsdl: ref },
  };
}
