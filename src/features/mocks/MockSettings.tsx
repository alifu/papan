import type { MEnv } from "./mockModel";
import { HeadersEditor } from "./HeadersEditor";

export interface MockSettingsProps {
  readonly mock: MEnv;
  readonly onChange: (mock: MEnv) => void;
}

export function MockSettings({ mock, onChange }: MockSettingsProps) {
  return (
    <fieldset className="mock-settings">
      <legend>Environment</legend>
      <div className="mock-row">
        <label>
          Name
          <input
            aria-label="Mock name"
            value={mock.name}
            onChange={(e) => onChange({ ...mock, name: e.target.value })}
          />
        </label>
        <label>
          Port (0 = any free port)
          <input
            aria-label="Port"
            inputMode="numeric"
            value={mock.port}
            onChange={(e) =>
              onChange({ ...mock, port: Number(e.target.value) || 0 })
            }
          />
        </label>
        <label>
          Prefix
          <input
            aria-label="Prefix"
            value={mock.prefix}
            placeholder="api/v1"
            onChange={(e) => onChange({ ...mock, prefix: e.target.value })}
          />
        </label>
        <label>
          Latency (ms)
          <input
            aria-label="Latency"
            inputMode="numeric"
            value={mock.latency_ms}
            onChange={(e) =>
              onChange({
                ...mock,
                latency_ms: Math.max(0, Number(e.target.value) || 0),
              })
            }
          />
        </label>
      </div>
      <div className="mock-row">
        <label>
          <input
            type="checkbox"
            aria-label="Allow all interfaces"
            checked={mock.bind === "all_interfaces"}
            onChange={(e) =>
              onChange({
                ...mock,
                bind: e.target.checked ? "all_interfaces" : "loopback",
              })
            }
          />
          Listen on all network interfaces (default: this computer only)
        </label>
        <label>
          <input
            type="checkbox"
            aria-label="HTTPS"
            checked={mock.tls}
            onChange={(e) => onChange({ ...mock, tls: e.target.checked })}
          />
          HTTPS with a self-signed certificate (clients must trust it)
        </label>
        <label>
          <input
            type="checkbox"
            aria-label="CORS"
            checked={mock.cors}
            onChange={(e) => onChange({ ...mock, cors: e.target.checked })}
          />
          Answer CORS requests
        </label>
      </div>
      <label>
        Proxy unmatched requests to (empty = answer 404)
        <input
          aria-label="Proxy target"
          value={mock.proxy}
          placeholder="https://api.example.com"
          onChange={(e) => onChange({ ...mock, proxy: e.target.value })}
        />
      </label>
      <HeadersEditor
        label="Global header"
        headers={mock.headers}
        onChange={(headers) => onChange({ ...mock, headers: [...headers] })}
      />
    </fieldset>
  );
}
