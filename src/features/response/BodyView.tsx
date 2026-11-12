import { useEffect, useMemo, useState } from "react";
import type { ResponseBody, ResponseMeta } from "../../ipc/bindings";
import { isAppError } from "../../ipc/errors";
import { Button } from "../../ui/Button";
import { reportError, showToast } from "../../ui/toastStore";
import { readResponseBody, saveResponseBody } from "./api";
import { availableModes, detectKind, type BodyKind, type BodyMode } from "./detect";
import { prettyJson, prettyXml } from "./format";
import { HtmlPreview } from "./HtmlPreview";
import { JsonTree } from "./JsonTree";
import { SearchBar } from "./SearchBar";
import { findMatches, toSegments } from "./search";
import { copyText } from "./clipboard";
import "./response.css";
import "./viewer.css";

const MODE_LABELS: Readonly<Record<BodyMode, string>> = {
  pretty: "Pretty",
  raw: "Raw",
  tree: "Tree",
  preview: "Preview",
};

type Load =
  | { readonly status: "loading" }
  | { readonly status: "ready"; readonly body: ResponseBody }
  | { readonly status: "gone" }
  | { readonly status: "failed" };

export interface BodyViewProps {
  readonly response: ResponseMeta;
}

const prettyText = (kind: BodyKind, text: string): string =>
  (kind === "json" ? prettyJson(text) : kind === "xml" ? prettyXml(text) : null) ?? text;

function HighlightedText({ text, current, matches }: {
  readonly text: string;
  readonly current: number;
  readonly matches: ReturnType<typeof findMatches>["matches"];
}) {
  const segments = useMemo(() => toSegments(text, matches), [text, matches]);
  return (
    <pre className="body-text">
      {segments.map((seg, i) =>
        seg.match === null ? (
          seg.text
        ) : (
          <mark key={i} className={seg.match === current ? "search-mark search-mark-current" : "search-mark"}>
            {seg.text}
          </mark>
        ),
      )}
    </pre>
  );
}

/** The body of one response: loads the capped preview and offers pretty/raw/tree/preview. */
export function BodyView({ response }: BodyViewProps) {
  const { request_id: requestId } = response;
  const [load, setLoad] = useState<Load>({ status: "loading" });
  const [mode, setMode] = useState<BodyMode | null>(null);
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState(0);

  useEffect(() => {
    let live = true;
    setLoad({ status: "loading" });
    setMode(null);
    setQuery("");
    setCurrent(0);
    readResponseBody(requestId).then(
      (body) => live && setLoad({ status: "ready", body }),
      (err: unknown) => live && setLoad({ status: isAppError(err) && err.code === "not_found" ? "gone" : "failed" }),
    );
    return () => {
      live = false;
    };
  }, [requestId]);

  const body = load.status === "ready" ? load.body : null;
  const text = body?.encoding === "text" ? body.data : "";
  const kind: BodyKind = body?.encoding === "base64" ? "image" : detectKind(response.content_type, text);
  const modes = availableModes(kind);
  const active: BodyMode = mode && modes.includes(mode) ? mode : (modes[0] ?? "raw");
  const shown = active === "pretty" ? prettyText(kind, text) : text;
  const searchable = active === "pretty" || active === "raw";
  const { matches, capped } = useMemo(
    () => (searchable ? findMatches(shown, query) : { matches: [], capped: false }),
    [searchable, shown, query],
  );

  const copy = () => {
    copyText(shown).then(
      () => showToast({ kind: "info", message: "Copied to clipboard." }),
      reportError,
    );
  };
  const save = () => {
    saveResponseBody(requestId).then((name) => {
      if (name !== null) showToast({ kind: "info", message: `Saved ${name}.` });
    }, reportError);
  };

  if (load.status === "loading") return <p className="response-notice">Loading body...</p>;
  if (load.status === "gone") return <p className="response-notice">The response body is no longer available. Send the request again.</p>;
  if (load.status === "failed" || body === null) return <p className="response-error">Could not read the response body.</p>;

  const mime = (response.content_type ?? "image/png").split(";")[0]?.trim() ?? "image/png";
  const noPreview = body.encoding === "none" && response.size_bytes > 0;

  return (
    <div className="body-view">
      <div className="body-toolbar" role="toolbar" aria-label="Body actions">
        {modes.map((m) => (
          <Button key={m} label={MODE_LABELS[m]} pressed={m === active} onClick={() => setMode(m)} />
        ))}
        <Button label="Copy" disabled={text === ""} onClick={copy} />
        <Button label="Save to file..." onClick={save} />
      </div>
      {searchable ? (
        <SearchBar
          query={query}
          onQueryChange={(q) => {
            setQuery(q);
            setCurrent(0);
          }}
          matchCount={matches.length}
          capped={capped}
          current={current}
          onCurrentChange={setCurrent}
        />
      ) : null}
      {(response.truncated || body.truncated) && (
        <p className="response-notice" role="status">
          {response.truncated
            ? "The body was cut at the size limit. Save to file keeps what was received."
            : "Only the start of the body is shown. Save to file keeps all of it."}
        </p>
      )}
      {noPreview ? <p className="response-notice">No preview for this content type. Use Save to file.</p> : null}
      {active === "tree" ? <JsonTree text={text} /> : null}
      {active === "preview" && kind === "html" ? <HtmlPreview html={text} /> : null}
      {active === "preview" && kind === "image" && body.encoding === "base64" ? (
        <img className="body-image" alt="Response" src={`data:${mime};base64,${body.data}`} />
      ) : null}
      {searchable ? <HighlightedText text={shown} current={current} matches={matches} /> : null}
    </div>
  );
}
