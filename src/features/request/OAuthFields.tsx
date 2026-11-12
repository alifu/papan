import { useState } from "react";
import { z } from "zod";
import type { Auth, ClientAuth, EffectiveVariable, OAuthGrant, RequestLocation } from "../../ipc/bindings";
import { Plain, Secret } from "./AuthFields";

export type OAuth2 = Extract<Auth, { type: "oauth2" }>;

export const OAUTH_DEFAULT: OAuth2 = {
  type: "oauth2",
  grant: "client_credentials",
  auth_url: "",
  token_url: "",
  client_id: "",
  client_secret: "",
  scope: "",
  client_auth: "basic",
  redirect_port: 0,
};

const MIN_PORT = 1024;
const MAX_PORT = 65535;
export const PORT_ERROR = `Port must be 0 (any free port) or ${MIN_PORT} to ${MAX_PORT}.`;
const portSchema = z.coerce
  .number()
  .int()
  .refine((n) => n === 0 || (n >= MIN_PORT && n <= MAX_PORT));

export interface OAuthFieldsProps {
  readonly auth: OAuth2;
  readonly onChange: (auth: Auth) => void;
  readonly location?: RequestLocation;
  readonly variables?: readonly EffectiveVariable[];
}

export function OAuthFields({ auth, onChange, location, variables }: OAuthFieldsProps) {
  const [portText, setPortText] = useState(String(auth.redirect_port));
  const [portError, setPortError] = useState(false);
  const set = (patch: Partial<OAuth2>) => onChange({ ...auth, ...patch });
  const editPort = (text: string) => {
    setPortText(text);
    const parsed = text.trim() === "" ? null : portSchema.safeParse(text);
    setPortError(!parsed?.success);
    if (parsed?.success) set({ redirect_port: parsed.data });
  };
  const common = { variables };
  return (
    <>
      <label>
        Grant type
        <select
          aria-label="Grant type"
          value={auth.grant}
          onChange={(e) => set({ grant: e.target.value as OAuthGrant })}
        >
          <option value="client_credentials">Client credentials</option>
          <option value="authorization_code">Authorization code (PKCE)</option>
        </select>
      </label>
      {auth.grant === "authorization_code" ? (
        <Plain label="Authorization URL" value={auth.auth_url} {...common} onChange={(auth_url) => set({ auth_url })} />
      ) : null}
      <Plain label="Token URL" value={auth.token_url} {...common} onChange={(token_url) => set({ token_url })} />
      <Plain label="Client ID" value={auth.client_id} {...common} onChange={(client_id) => set({ client_id })} />
      <Secret
        label="Client secret"
        value={auth.client_secret}
        {...common}
        location={location}
        defaultName="client_secret"
        onChange={(client_secret) => set({ client_secret })}
      />
      <Plain label="Scope" value={auth.scope} {...common} onChange={(scope) => set({ scope })} />
      <label>
        Client authentication
        <select
          aria-label="Client authentication"
          value={auth.client_auth}
          onChange={(e) => set({ client_auth: e.target.value as ClientAuth })}
        >
          <option value="basic">Basic header</option>
          <option value="body">Credentials in body</option>
        </select>
      </label>
      {auth.grant === "authorization_code" ? (
        <label>
          Redirect port (0 = any free port)
          <input inputMode="numeric" aria-invalid={portError} value={portText} onChange={(e) => editPort(e.target.value)} />
        </label>
      ) : null}
      {portError ? <p role="alert" className="field-error">{PORT_ERROR}</p> : null}
    </>
  );
}
