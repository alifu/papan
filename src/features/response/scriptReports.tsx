import type { ScriptError, ScriptPhase, ScriptReport } from "../../ipc/bindings";

const PHASE_LABEL: Record<ScriptPhase, string> = { pre_request: "pre-request", post_response: "post-response" };

/** The first script error across the reports (pre-request runs first). */
export const firstScriptError = (scripts: readonly ScriptReport[]): ScriptError | null =>
  scripts.find((report) => report.error)?.error ?? null;

export const testCounts = (scripts: readonly ScriptReport[]): { passed: number; total: number } => {
  const tests = scripts.flatMap((report) => report.tests);
  return { passed: tests.filter((t) => t.passed).length, total: tests.length };
};

export function ScriptErrorBanner({ error }: { readonly error: ScriptError }) {
  const where = [error.script, error.line !== null ? `line ${error.line}` : null].filter(Boolean).join(", ");
  return (
    <p className="script-error" role="alert">
      <strong>Script {error.kind.replace(/_/g, " ")}</strong>: {error.message}
      {where ? <span className="script-error-where"> ({where})</span> : null}
    </p>
  );
}

export function ConsolePane({ scripts }: { readonly scripts: readonly ScriptReport[] }) {
  const empty = scripts.every((r) => r.console.length === 0 && r.changes.length === 0 && !r.console_truncated);
  if (empty) return <p className="response-notice">No console output.</p>;
  return (
    <div className="script-console" role="log" aria-label="Script console">
      {scripts.map((report) => (
        <div key={report.phase}>
          {report.console.map((line, i) => (
            <div key={i} className={`console-line console-${line.level}`}>
              <span className="console-phase">[{PHASE_LABEL[report.phase]}]</span> <span>{line.level}</span> {line.text}
            </div>
          ))}
          {report.console_truncated ? <p className="response-notice">Output truncated.</p> : null}
          {report.changes.map((c, i) => (
            <div key={`c${i}`} className="console-line console-change">
              <span className="console-phase">[{PHASE_LABEL[report.phase]}]</span>{" "}
              {c.removed ? `Unset ${c.scope} variable ${c.name}` : `Set ${c.scope} variable ${c.name}${c.secret || c.value === null ? "" : ` = ${c.value}`}`}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

export function TestsPane({ scripts }: { readonly scripts: readonly ScriptReport[] }) {
  const tests = scripts.flatMap((report) => report.tests);
  if (tests.length === 0) return <p className="response-notice">No tests ran.</p>;
  return (
    <ul className="script-tests" aria-label="Test results">
      {tests.map((t, i) => (
        <li key={i} className={t.passed ? "test-pass" : "test-fail"}>
          <strong>{t.passed ? "PASS" : "FAIL"}</strong> {t.name}
          {t.error ? <div className="test-error">{t.error}</div> : null}
        </li>
      ))}
    </ul>
  );
}
