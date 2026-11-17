import { useEffect, useMemo, useState } from "react";
import { graphql } from "cm6-graphql";
import type { Body, EffectiveVariable, RequestContent, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { graphqlTab, useGraphqlStore, type GraphqlTabState } from "./graphqlStore";
import { listOperations } from "./operations";
import { toSchema } from "./schema";
import { SchemaExplorer } from "./SchemaExplorer";

export type GraphqlBody = Extract<Body, { type: "graphql" }>;

export interface GraphqlEditorProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
  readonly variables?: readonly EffectiveVariable[];
  readonly onChange: (body: GraphqlBody) => void;
}

const EMPTY_BODY: GraphqlBody = { type: "graphql", query: "", variables: "", operation_name: "" };

/** The GraphQL body of a request; a request without one reads as an empty body. */
export const graphqlBodyOf = (content: RequestContent): GraphqlBody =>
  content.body.type === "graphql" ? content.body : EMPTY_BODY;

const timeOf = (ms: number): string => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

function statusText(state: GraphqlTabState, schemaError: Error | null): string {
  if (state.status === "loading") return "Fetching schema…";
  if (state.status === "error") return state.error ?? "The schema could not be loaded.";
  if (schemaError) return `The schema could not be read: ${schemaError.message}`;
  if (state.status === "ready") return state.fetchedAt === null ? "Schema from file" : `Schema from ${timeOf(state.fetchedAt)}`;
  return "No schema loaded";
}

interface SchemaBarProps {
  readonly state: GraphqlTabState;
  readonly schemaError: Error | null;
  readonly explorerOpen: boolean;
  readonly onExplorer: () => void;
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
}

function SchemaBar({ state, schemaError, explorerOpen, onExplorer, tabId, location, content }: SchemaBarProps) {
  const { introspect, loadFile, clear, cancel } = useGraphqlStore.getState();
  const hasSchema = state.schema !== null && schemaError === null;
  return (
    <div className="graphql-schema-bar" role="toolbar" aria-label="Schema">
      <span className="graphql-schema-status" role="status" title={state.endpoint || undefined}>
        {statusText(state, schemaError)}
      </span>
      {state.status === "loading" ? (
        <Button label="Cancel" onClick={() => cancel(tabId)} />
      ) : (
        <Button label={state.schema ? "Refresh" : "Fetch schema"} onClick={() => void introspect(tabId, location, content)} />
      )}
      <Button label="Load from file…" disabled={state.status === "loading"} onClick={() => void loadFile(tabId, location, content)} />
      {state.schema ? <Button label="Clear schema" onClick={() => void clear(tabId, location, content)} /> : null}
      <Button label="Explorer" pressed={explorerOpen} disabled={!hasSchema} onClick={onExplorer} />
    </div>
  );
}

function OperationPicker({ body, onChange }: { readonly body: GraphqlBody; readonly onChange: (name: string) => void }) {
  const names = listOperations(body.query).map((o) => o.name).filter((n) => n !== "");
  if (names.length < 2) return null;
  const options = names.includes(body.operation_name) || body.operation_name === "" ? names : [body.operation_name, ...names];
  return (
    <label>
      Operation
      <select aria-label="Operation" value={body.operation_name} onChange={(e) => onChange(e.target.value)}>
        <option value="">Select an operation…</option>
        {options.map((name) => (
          <option key={name} value={name}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Query editor with schema-aware completion and lint, operation picker, schema status line and explorer. */
export function GraphqlEditor({ tabId, location, content, variables, onChange }: GraphqlEditorProps) {
  const body = graphqlBodyOf(content);
  const state = useGraphqlStore((s) => graphqlTab(s, tabId));
  const [explorerOpen, setExplorerOpen] = useState(false);
  const locationKey = JSON.stringify(location);

  useEffect(() => {
    void useGraphqlStore.getState().loadCached(tabId, location, content);
    // Opening (or switching to) a request reads its cache once; later edits need an explicit Refresh.
  }, [tabId, locationKey]);

  const built = useMemo(() => (state.schema ? toSchema(state.schema) : null), [state.schema]);
  const schema = built instanceof Error ? null : built;
  const schemaError = built instanceof Error ? built : null;
  // Stable between renders: the editor reconfigures its compartment whenever this value changes.
  const extensions = useMemo(() => graphql(schema ?? undefined), [schema]);

  return (
    <div className="graphql-editor">
      <SchemaBar
        state={state}
        schemaError={schemaError}
        explorerOpen={explorerOpen && schema !== null}
        onExplorer={() => setExplorerOpen((open) => !open)}
        tabId={tabId}
        location={location}
        content={content}
      />
      <OperationPicker body={body} onChange={(operation_name) => onChange({ ...body, operation_name })} />
      <div className="graphql-main">
        <div className="body-text">
          <CodeEditor
            language="text"
            ariaLabel="GraphQL query"
            value={body.query}
            variables={variables}
            extensions={extensions}
            onChange={(query) => onChange({ ...body, query })}
          />
        </div>
        {explorerOpen && schema ? <SchemaExplorer schema={schema} /> : null}
      </div>
    </div>
  );
}
