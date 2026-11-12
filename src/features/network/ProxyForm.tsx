import { useState } from "react";
import type { NetworkSettings, ProxyInput, ProxyMode, SecretEdit } from "../../ipc/bindings";
import { Button } from "../../ui/Button";

export const MAX_BYPASS = 100;
const URL_ERROR = "Enter an http:// or https:// proxy address with a host, and no user name, path or query.";

export interface ProxyFormProps {
  readonly proxy: NetworkSettings["proxy"];
  readonly onSave: (proxy: ProxyInput) => Promise<boolean>;
}

/** Whether `text` is an http(s) URL with a host and no userinfo, path, query or fragment. */
export function validProxyUrl(text: string): boolean {
  try {
    const url = new URL(text.trim());
    return (
      (url.protocol === "http:" || url.protocol === "https:") &&
      url.hostname !== "" &&
      url.username === "" &&
      url.password === "" &&
      url.search === "" &&
      url.hash === "" &&
      url.pathname === "/"
    );
  } catch {
    return false;
  }
}

const parseBypass = (text: string): string[] =>
  text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);

export function ProxyForm({ proxy, onSave }: ProxyFormProps) {
  const [mode, setMode] = useState<ProxyMode>(proxy.mode);
  const [url, setUrl] = useState(proxy.url);
  const [username, setUsername] = useState(proxy.username);
  const [password, setPassword] = useState("");
  const [cleared, setCleared] = useState(false);
  const [bypass, setBypass] = useState(proxy.bypass.join("\n"));
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    const list = parseBypass(bypass);
    if ((mode === "custom" || url.trim() !== "") && !validProxyUrl(url)) return setError(URL_ERROR);
    if (list.length > MAX_BYPASS) return setError(`At most ${MAX_BYPASS} bypass entries.`);
    setError(null);
    const edit: SecretEdit = cleared ? { op: "clear" } : password !== "" ? { op: "set", value: password } : { op: "keep" };
    if (await onSave({ mode, url: url.trim(), username, password: edit, bypass: list })) {
      setPassword("");
      setCleared(false);
    }
  };

  const custom = mode === "custom";
  return (
    <fieldset className="editor-form">
      <legend>Proxy</legend>
      {(["system", "none", "custom"] as const).map((m) => (
        <label key={m}>
          <input type="radio" name="proxy-mode" checked={mode === m} onChange={() => setMode(m)} />
          {m === "system" ? "Use system proxy" : m === "none" ? "No proxy" : "Custom proxy"}
        </label>
      ))}
      <label>
        Proxy URL
        <input spellCheck={false} disabled={!custom} value={url} onChange={(e) => setUrl(e.target.value)} />
      </label>
      <label>
        Username
        <input spellCheck={false} disabled={!custom} value={username} onChange={(e) => setUsername(e.target.value)} />
      </label>
      <label>
        Password
        <input
          type="password"
          disabled={!custom}
          placeholder={cleared ? "Will be cleared" : proxy.has_password ? "Stored" : "Not set"}
          value={password}
          onChange={(e) => {
            setPassword(e.target.value);
            setCleared(false);
          }}
        />
      </label>
      <Button
        label="Clear password"
        disabled={!custom || (!proxy.has_password && !cleared) || cleared}
        onClick={() => {
          setPassword("");
          setCleared(true);
        }}
      />
      <label>
        Bypass hosts (one per line)
        <textarea rows={3} spellCheck={false} disabled={!custom} value={bypass} onChange={(e) => setBypass(e.target.value)} />
      </label>
      {error ? <p role="alert" className="field-error">{error}</p> : null}
      <Button label="Save proxy" onClick={() => void submit()} />
    </fieldset>
  );
}
