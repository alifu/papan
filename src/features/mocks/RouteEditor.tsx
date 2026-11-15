import type { ResponseMode } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { HeadersEditor } from "./HeadersEditor";
import {
  METHODS,
  MODES,
  newResponse,
  replaceAt,
  withDefault,
  withoutResponse,
  type MResponse,
  type MRoute,
} from "./mockModel";
import { RulesEditor } from "./RulesEditor";

export const MAX_RESPONSES = 50;

export interface RouteEditorProps {
  readonly route: MRoute;
  readonly onChange: (route: MRoute) => void;
}

function ResponseEditor({
  response,
  index,
  route,
  onChange,
}: {
  readonly response: MResponse;
  readonly index: number;
  readonly route: MRoute;
  readonly onChange: (route: MRoute) => void;
}) {
  const set = (next: MResponse) =>
    onChange({ ...route, responses: replaceAt(route.responses, index, next) });
  const n = index + 1;
  return (
    <fieldset className="mock-response">
      <legend>
        Response {n}
        {response.default ? " (default)" : ""}
      </legend>
      <div className="mock-row">
        <label>
          Status
          <input
            aria-label={`Response ${n} status`}
            inputMode="numeric"
            value={response.status}
            onChange={(e) =>
              set({ ...response, status: Number(e.target.value) || 0 })
            }
          />
        </label>
        <label>
          Label
          <input
            aria-label={`Response ${n} label`}
            value={response.label}
            onChange={(e) => set({ ...response, label: e.target.value })}
          />
        </label>
        <label>
          Latency (ms)
          <input
            aria-label={`Response ${n} latency`}
            inputMode="numeric"
            value={response.latency_ms}
            onChange={(e) =>
              set({
                ...response,
                latency_ms: Math.max(0, Number(e.target.value) || 0),
              })
            }
          />
        </label>
        <label>
          <input
            type="radio"
            name={`default-${route.id}`}
            aria-label={`Response ${n} is the default`}
            checked={response.default}
            onChange={() => onChange(withDefault(route, index))}
          />
          Default
        </label>
        <Button
          icon="close"
          aria-label={`Remove response ${n}`}
          disabled={route.responses.length <= 1}
          onClick={() => onChange(withoutResponse(route, index))}
        />
      </div>
      <HeadersEditor
        label={`Response ${n} header`}
        headers={response.headers}
        onChange={(headers) => set({ ...response, headers })}
      />
      <label>
        File (path inside the collection; sent instead of the body)
        <input
          aria-label={`Response ${n} file`}
          value={response.file}
          placeholder="files/users.json"
          onChange={(e) => set({ ...response, file: e.target.value })}
        />
      </label>
      <label>
        Body (Handlebars: {"{{urlParam 'id'}}"}, {"{{body 'a.b'}}"},{" "}
        {"{{uuid}}"} …)
        <textarea
          aria-label={`Response ${n} body`}
          value={response.body}
          rows={6}
          spellCheck={false}
          onChange={(e) => set({ ...response, body: e.target.value })}
        />
      </label>
      <RulesEditor response={response} onChange={set} />
    </fieldset>
  );
}

export function RouteEditor({ route, onChange }: RouteEditorProps) {
  if (route.kind === "crud") {
    return (
      <div className="mock-route">
        <p className="empty-text">
          Data route: serves the data bucket “{route.bucket}” at /
          {route.endpoint} and /{route.endpoint}/:id. Edit it in the mock’s
          .mock.yaml file for now.
        </p>
      </div>
    );
  }
  return (
    <div className="mock-route">
      <div className="mock-row">
        <select
          aria-label="Method"
          value={route.method}
          onChange={(e) => onChange({ ...route, method: e.target.value })}
        >
          {METHODS.map((m) => (
            <option key={m} value={m}>
              {m.toUpperCase()}
            </option>
          ))}
        </select>
        <input
          aria-label="Endpoint"
          value={route.endpoint}
          placeholder="users/:id"
          onChange={(e) => onChange({ ...route, endpoint: e.target.value })}
        />
        <label>
          <input
            type="checkbox"
            aria-label="Route enabled"
            checked={route.enabled}
            onChange={(e) => onChange({ ...route, enabled: e.target.checked })}
          />
          Enabled
        </label>
        <label>
          Mode
          <select
            aria-label="Response mode"
            value={route.mode}
            onChange={(e) =>
              onChange({ ...route, mode: e.target.value as ResponseMode })
            }
          >
            {MODES.map((m) => (
              <option key={m.value} value={m.value}>
                {m.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {route.responses.map((response, index) => (
        <ResponseEditor
          key={response.id}
          response={response}
          index={index}
          route={route}
          onChange={onChange}
        />
      ))}
      <Button
        icon="plus"
        label="Add response"
        disabled={route.responses.length >= MAX_RESPONSES}
        onClick={() =>
          onChange({
            ...route,
            responses: [...route.responses, newResponse(false)],
          })
        }
      />
    </div>
  );
}
