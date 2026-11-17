import { useEffect, useState } from "react";
import type { OAuthStatus, RequestContent, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { scopeKey, useEnvStore } from "../environments/envStore";
import { tabState, useOAuthStore } from "./oauthStore";
import { useRequestStore } from "./requestStore";

export interface OAuthTokenPanelProps {
  readonly tabId: string;
  readonly location: RequestLocation;
  readonly content: RequestContent;
}

const REFRESH_DEBOUNCE_MS = 300;
/** The engine refreshes a token this long before it expires; the UI shows it as expired from then on. */
export const EXPIRY_SKEW_MS = 30_000;

/** The one line shown for a status; the clock is injected for tests. */
export function statusText(status: OAuthStatus | null, now: number = Date.now()): string {
  if (!status?.has_token) return "No token";
  if (status.expires_at_ms === null) return "Token valid";
  if (status.expires_at_ms - EXPIRY_SKEW_MS <= now) return status.can_refresh ? "Expired, will refresh" : "Expired";
  const time = new Date(status.expires_at_ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Token valid until ${time}`;
}

function AuthorizeDialog({ status, onConfirm, onClose }: {
  readonly status: OAuthStatus;
  readonly onConfirm: () => void;
  readonly onClose: () => void;
}) {
  return (
    <Dialog open title="Get new token" onClose={onClose} actions={<Button label="Get token" onClick={onConfirm} />}>
      {status.auth_host ? (
        <p className="dialog-message">
          Opens <strong>{status.auth_host}</strong> in your browser.
        </p>
      ) : null}
      <p className="dialog-message">
        Sends the client secret to <strong>{status.token_host}</strong>.
      </p>
      {status.plaintext ? (
        <p className="dialog-message" role="alert">
          Warning: a URL uses plain http, so the exchange is not encrypted.
        </p>
      ) : null}
      {status.verify_tls ? null : (
        <p className="dialog-message" role="alert">
          TLS certificate verification is off: the client secret and tokens are sent without certificate checks.
        </p>
      )}
      {status.redirect_uri ? (
        <p className="dialog-message">
          Register this redirect URI with the provider: <code>{status.redirect_uri}</code>
        </p>
      ) : null}
    </Dialog>
  );
}

/** Token state of the draft's OAuth 2 configuration, with Get new token / Clear token. */
export function OAuthTokenPanel({ tabId, location, content }: OAuthTokenPanelProps) {
  const tab = useOAuthStore((s) => tabState(s, tabId));
  const { refresh, authorize, cancel, clear } = useOAuthStore.getState();
  const [confirming, setConfirming] = useState<OAuthStatus | null>(null);
  const pending = tab.flowId !== null;
  // Resolved hosts and `config_key` follow the active environment and variables.
  const variables = useEnvStore((s) => s.effective[scopeKey(location)]?.variables);
  // A send may refresh the token behind the panel's back.
  const sending = useRequestStore((s) => s.byTab[tabId]?.sending ?? false);
  const [, setTick] = useState(0);
  const expiresAt = tab.status?.has_token ? tab.status.expires_at_ms : null;
  // The status depends on the configuration only; other edits must not trigger a call.
  const authKey = JSON.stringify(content.auth);

  useEffect(() => {
    if (pending) return;
    const timer = setTimeout(() => void refresh(tabId, location, content), REFRESH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // `content` is covered by `authKey`; `location` by its identity per tab.
  }, [tabId, authKey, pending, variables, sending]);

  // One re-render when the token turns expired.
  useEffect(() => {
    if (expiresAt === null) return;
    const wait = expiresAt - EXPIRY_SKEW_MS - Date.now();
    if (wait <= 0) return;
    const timer = setTimeout(() => setTick((n) => n + 1), wait);
    return () => clearTimeout(timer);
  }, [expiresAt]);

  const openConfirm = async () => {
    // Fresh status so the dialog shows the hosts and `config_key` as they resolve right now.
    const fresh = await refresh(tabId, location, content, true);
    if (fresh) setConfirming(fresh);
  };
  const confirm = () => {
    if (!confirming) return;
    void authorize(tabId, location, content, confirming.config_key);
    setConfirming(null);
  };

  return (
    <div className="oauth-panel">
      <p role="status" className="oauth-status">
        {pending ? "Waiting for the browser…" : (tab.notice ?? (tab.incomplete ? "Configuration incomplete" : statusText(tab.status)))}
      </p>
      <div className="oauth-actions">
        {pending ? (
          <Button label="Cancel" onClick={() => cancel(tabId)} />
        ) : (
          <>
            <Button label="Get new token" onClick={() => void openConfirm()} />
            <Button
              label="Clear token"
              disabled={!tab.status?.has_token}
              onClick={() => void clear(tabId, location, content)}
            />
          </>
        )}
      </div>
      {confirming ? <AuthorizeDialog status={confirming} onConfirm={confirm} onClose={() => setConfirming(null)} /> : null}
    </div>
  );
}
