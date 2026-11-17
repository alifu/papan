import type { ApiKeyPlacement, Auth, EffectiveVariable, RequestLocation } from "../../ipc/bindings";
import { Plain, Secret, SECRET_HINT } from "./AuthFields";
import { OAuthFields, OAUTH_DEFAULT } from "./OAuthFields";

export { SECRET_HINT };

export interface AuthEditorProps {
  readonly auth: Auth;
  readonly onChange: (auth: Auth) => void;
  /** Enables "Save as secret variable…"; omitted when there is nowhere to save. */
  readonly location?: RequestLocation;
  readonly variables?: readonly EffectiveVariable[];
  /** gRPC: no Digest, and API keys go in metadata only. */
  readonly grpc?: boolean;
}

const DEFAULTS: Record<string, Auth> = {
  none: { type: "none" },
  basic: { type: "basic", username: "", password: "" },
  bearer: { type: "bearer", token: "" },
  api_key: { type: "api_key", name: "", value: "", placement: "header" },
  digest: { type: "digest", username: "", password: "" },
  oauth2: OAUTH_DEFAULT,
};

export function AuthEditor({ auth, onChange, location, variables, grpc = false }: AuthEditorProps) {
  if (auth.type === "unsupported") {
    return <p className="empty-text">Made by a newer Papan. This authentication is read-only.</p>;
  }
  return (
    <div className="editor-form">
      <label>
        Type
        <select
          aria-label="Auth type"
          value={auth.type}
          onChange={(e) => onChange(DEFAULTS[e.target.value] ?? DEFAULTS.none)}
        >
          <option value="none">None</option>
          <option value="basic">Basic</option>
          <option value="bearer">Bearer token</option>
          <option value="api_key">API key</option>
          {grpc ? null : <option value="digest">Digest</option>}
          <option value="oauth2">OAuth 2</option>
        </select>
      </label>
      {auth.type === "basic" ? (
        <>
          <Plain label="Username" value={auth.username} variables={variables} onChange={(username) => onChange({ ...auth, username })} />
          <Secret
            label="Password"
            value={auth.password}
            variables={variables}
            location={location}
            defaultName="password"
            onChange={(password) => onChange({ ...auth, password })}
          />
        </>
      ) : null}
      {auth.type === "digest" ? (
        <>
          <Plain label="Username" value={auth.username} variables={variables} onChange={(username) => onChange({ ...auth, username })} />
          <Secret
            label="Password"
            value={auth.password}
            variables={variables}
            location={location}
            defaultName="password"
            onChange={(password) => onChange({ ...auth, password })}
          />
        </>
      ) : null}
      {auth.type === "oauth2" ? <OAuthFields auth={auth} onChange={onChange} location={location} variables={variables} /> : null}
      {auth.type === "bearer" ? (
        <Secret
          label="Token"
          value={auth.token}
          variables={variables}
          location={location}
          defaultName="token"
          onChange={(token) => onChange({ ...auth, token })}
        />
      ) : null}
      {auth.type === "api_key" ? (
        <>
          <Plain label="Key name" value={auth.name} variables={variables} onChange={(name) => onChange({ ...auth, name })} />
          <Secret
            label="Key value"
            value={auth.value}
            variables={variables}
            location={location}
            defaultName="api_key"
            onChange={(value) => onChange({ ...auth, value })}
          />
          {grpc ? null : (
            <label>
              Add to
              <select
                aria-label="Add to"
                value={auth.placement}
                onChange={(e) => onChange({ ...auth, placement: e.target.value as ApiKeyPlacement })}
              >
                <option value="header">Header</option>
                <option value="query">Query parameter</option>
              </select>
            </label>
          )}
        </>
      ) : null}
    </div>
  );
}
