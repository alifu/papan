import type { ResponseMode } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import {
  MODES,
  newResponse,
  replaceAt,
  withDefault,
  withoutResponse,
  type MRoute,
} from "./mockModel";
import { RulesEditor } from "./RulesEditor";

const MAX_REPLIES = 50;

export interface WsRouteEditorProps {
  readonly route: MRoute;
  readonly onChange: (route: MRoute) => void;
}

/** A WebSocket route: a greeting on connect, then a reply per message the rules choose. */
export function WsRouteEditor({ route, onChange }: WsRouteEditorProps) {
  return (
    <div className="mock-route">
      <p className="empty-text">
        A WebSocket route accepts connections at the endpoint. It sends the
        greeting when a client connects, then answers each text message with the
        reply the rules choose. In a reply, {"{{body}}"} is the whole message
        and {"{{body 'a.b'}}"} reads its JSON.
      </p>
      <div className="mock-row">
        <input
          aria-label="Endpoint"
          value={route.endpoint}
          placeholder="chat/:room"
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
      <label>
        Greeting (sent on connect; empty sends nothing)
        <textarea
          aria-label="Greeting"
          value={route.greeting}
          rows={3}
          spellCheck={false}
          onChange={(e) => onChange({ ...route, greeting: e.target.value })}
        />
      </label>
      {route.responses.map((reply, index) => {
        const n = index + 1;
        const set = (next: typeof reply) =>
          onChange({
            ...route,
            responses: replaceAt(route.responses, index, next),
          });
        return (
          <fieldset key={reply.id} className="mock-response">
            <legend>
              Reply {n}
              {reply.default ? " (default)" : ""}
            </legend>
            <div className="mock-row">
              <label>
                Label
                <input
                  aria-label={`Reply ${n} label`}
                  value={reply.label}
                  onChange={(e) => set({ ...reply, label: e.target.value })}
                />
              </label>
              <label>
                Delay (ms)
                <input
                  aria-label={`Reply ${n} delay`}
                  inputMode="numeric"
                  value={reply.latency_ms}
                  onChange={(e) =>
                    set({
                      ...reply,
                      latency_ms: Math.max(0, Number(e.target.value) || 0),
                    })
                  }
                />
              </label>
              <label>
                <input
                  type="radio"
                  name={`default-${route.id}`}
                  aria-label={`Reply ${n} is the default`}
                  checked={reply.default}
                  onChange={() => onChange(withDefault(route, index))}
                />
                Default
              </label>
              <Button
                icon="close"
                aria-label={`Remove reply ${n}`}
                onClick={() => onChange(withoutResponse(route, index))}
              />
            </div>
            <label>
              Message (an empty reply sends nothing)
              <textarea
                aria-label={`Reply ${n} message`}
                value={reply.body}
                rows={4}
                spellCheck={false}
                onChange={(e) => set({ ...reply, body: e.target.value })}
              />
            </label>
            <RulesEditor response={reply} onChange={set} />
          </fieldset>
        );
      })}
      <Button
        icon="plus"
        label="Add reply"
        disabled={route.responses.length >= MAX_REPLIES}
        onClick={() =>
          onChange({
            ...route,
            responses: [
              ...route.responses,
              newResponse(route.responses.length === 0),
            ],
          })
        }
      />
    </div>
  );
}
