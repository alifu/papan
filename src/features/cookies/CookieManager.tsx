import { useEffect, useState } from "react";
import type { CookieFilter, CookieInput, RequestLocation, StoredCookie } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { reportError } from "../../ui/toastStore";
import { ConfirmDialog } from "../collections/ConfirmDialog";
import * as api from "./api";
import { CookieForm } from "./CookieForm";
import { cookieRows } from "./cookieRows";

export interface CookieManagerProps {
  readonly location: RequestLocation;
  readonly onClose: () => void;
}

type Prompt =
  | { readonly kind: "edit"; readonly cookie: StoredCookie | null }
  | { readonly kind: "clear" };

/** Cookies of the jar `location` uses, grouped by domain. */
export function CookieManager({ location, onClose }: CookieManagerProps) {
  const [cookies, setCookies] = useState<readonly StoredCookie[] | null>(null);
  const [filter, setFilter] = useState("");
  const [prompt, setPrompt] = useState<Prompt | null>(null);

  useEffect(() => {
    let active = true;
    api.listCookies(location).then(
      (list) => active && setCookies(list),
      (err) => {
        if (active) setCookies([]);
        reportError(err);
      },
    );
    return () => {
      active = false;
    };
  }, [location]);

  /** Runs a write that answers the jar's new list. */
  const apply = async (call: () => Promise<readonly StoredCookie[]>): Promise<boolean> => {
    try {
      setCookies(await call());
      return true;
    } catch (err) {
      reportError(err);
      return false;
    }
  };
  const remove = (filterBy: CookieFilter) => void apply(() => api.deleteCookies(location, filterBy));
  const save = (cookie: CookieInput) => apply(() => api.setCookie(location, cookie));
  const clearAll = () => {
    setPrompt(null);
    void apply(async () => {
      await api.clearCookies(location);
      return [];
    });
  };

  const groups = cookieRows(cookies ?? [], filter);
  return (
    <>
      <Dialog
        open
        title="Cookies"
        onClose={onClose}
        actions={
          <>
            <Button label="Add cookie…" onClick={() => setPrompt({ kind: "edit", cookie: null })} />
            <Button label="Clear all" disabled={!cookies?.length} onClick={() => setPrompt({ kind: "clear" })} />
          </>
        }
      >
        <input aria-label="Filter cookies" placeholder="Filter by name, domain or path" value={filter} onChange={(e) => setFilter(e.target.value)} />
        {cookies === null ? <p className="hint">Loading...</p> : null}
        {cookies !== null && groups.length === 0 ? <p className="hint">No cookies.</p> : null}
        {groups.map((group) => (
          <section key={group.domain} aria-label={`Cookies for ${group.domain}`}>
            <h4>
              {group.domain}{" "}
              <Button
                label="Delete domain"
                aria-label={`Delete all cookies of ${group.domain}`}
                onClick={() => remove({ domain: group.domain, path: null, name: null })}
              />
            </h4>
            <table className="response-table">
              <tbody>
                {group.rows.map(({ key, cookie, expires, flags }) => (
                  <tr key={key}>
                    <th scope="row">{cookie.name}</th>
                    <td>{cookie.value}</td>
                    <td>{cookie.path}</td>
                    <td>{expires}</td>
                    <td>{flags}</td>
                    <td>
                      <Button label="Edit" aria-label={`Edit cookie ${cookie.name}`} onClick={() => setPrompt({ kind: "edit", cookie })} />
                      <Button
                        label="Delete"
                        aria-label={`Delete cookie ${cookie.name}`}
                        onClick={() => remove({ domain: cookie.domain, path: cookie.path, name: cookie.name })}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        ))}
      </Dialog>
      {prompt?.kind === "edit" ? (
        <CookieForm initial={prompt.cookie} onSubmit={save} onClose={() => setPrompt(null)} />
      ) : null}
      {prompt?.kind === "clear" ? (
        <ConfirmDialog
          title="Clear all cookies?"
          message="Every cookie in this jar is deleted."
          confirmLabel="Clear all"
          onConfirm={clearAll}
          onClose={() => setPrompt(null)}
        />
      ) : null}
    </>
  );
}
