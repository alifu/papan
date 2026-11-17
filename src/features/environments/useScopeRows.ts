import { useCallback, useEffect, useState } from "react";
import type { VarScope } from "../../ipc/bindings";
import { reportError, showToast } from "../../ui/toastStore";
import * as api from "./api";
import { useEnvStore } from "./envStore";
import { isDirty, rowFromVariable, toInputs, validateRows, withTrailing, type VariableRow } from "./variableRows";

const toRows = (variables: Parameters<typeof rowFromVariable>[0][]): readonly VariableRow[] =>
  withTrailing(variables.map(rowFromVariable));

export interface ScopeRows {
  readonly rows: readonly VariableRow[];
  readonly errors: Readonly<Record<string, string>>;
  readonly loading: boolean;
  readonly dirty: boolean;
  setRows: (rows: readonly VariableRow[]) => void;
  save: () => Promise<boolean>;
}

/** Loads one scope's variables, tracks edits against the saved copy, and writes the whole scope. */
export function useScopeRows(scope: VarScope | null): ScopeRows {
  const [rows, setRows] = useState<readonly VariableRow[]>([]);
  const [saved, setSaved] = useState<readonly VariableRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<Readonly<Record<string, string>>>({});
  const scopeKey = scope ? JSON.stringify(scope) : "";

  useEffect(() => {
    setErrors({});
    if (!scope) {
      setRows([]);
      setSaved([]);
      return;
    }
    let live = true;
    setLoading(true);
    api.readVariables(scope).then(
      (variables) => {
        if (!live) return;
        const loaded = toRows(variables);
        setRows(loaded);
        setSaved(loaded);
        setLoading(false);
      },
      (err: unknown) => {
        if (!live) return;
        setLoading(false);
        reportError(err);
      },
    );
    return () => {
      live = false;
    };
    // `scopeKey` identifies `scope`.
  }, [scopeKey]);

  const save = useCallback(async (): Promise<boolean> => {
    if (!scope) return false;
    const found = validateRows(rows);
    setErrors(found);
    if (Object.keys(found).length > 0) return false;
    try {
      const variables = await api.writeVariables(scope, toInputs(rows));
      const next = toRows(variables);
      setRows(next);
      setSaved(next);
      await useEnvStore.getState().refreshEffective();
      showToast({ kind: "info", message: "Variables saved." });
      return true;
    } catch (err) {
      reportError(err);
      return false;
    }
  }, [rows, scopeKey]);

  return { rows, errors, loading, dirty: isDirty(rows, saved), setRows, save };
}
