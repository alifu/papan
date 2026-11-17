import type { Scripts } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { CodeEditor } from "../../ui/code/CodeEditor";
import { appendSnippet, SNIPPETS } from "./snippets";

export type ScriptPhase = "pre_request" | "post_response";

const PHASE_LABELS: Readonly<Record<ScriptPhase, string>> = {
  pre_request: "Pre-request",
  post_response: "Post-response",
};

export interface ScriptsEditorProps {
  readonly scripts: Scripts;
  readonly phase: ScriptPhase;
  readonly onPhase: (phase: ScriptPhase) => void;
  readonly onChange: (scripts: Scripts) => void;
}

/** Pre-request / post-response JavaScript editor with snippet buttons. */
export function ScriptsEditor({ scripts, phase, onPhase, onChange }: ScriptsEditorProps) {
  const setText = (text: string) => onChange({ ...scripts, [phase]: text });
  return (
    <div className="scripts-editor">
      <label className="scripts-enabled">
        <input
          type="checkbox"
          checked={scripts.enabled}
          onChange={(e) => onChange({ ...scripts, enabled: e.target.checked })}
        />{" "}
        Enabled
      </label>
      {!scripts.enabled && (
        <p role="note" className="scripts-disabled-note">
          Disabled: these scripts will not run. Review them before enabling.
        </p>
      )}
      <div className="scripts-toolbar" role="toolbar" aria-label="Script phase">
        {(Object.keys(PHASE_LABELS) as ScriptPhase[]).map((p) => (
          <Button key={p} label={PHASE_LABELS[p]} pressed={p === phase} onClick={() => onPhase(p)} />
        ))}
      </div>
      <div className="scripts-toolbar" role="toolbar" aria-label="Snippets">
        {SNIPPETS.map((s) => (
          <Button key={s.label} label={s.label} onClick={() => setText(appendSnippet(scripts[phase], s.code))} />
        ))}
      </div>
      <div className="scripts-body">
        <CodeEditor
          language="javascript"
          ariaLabel={`${PHASE_LABELS[phase]} script`}
          value={scripts[phase]}
          onChange={setText}
        />
      </div>
    </div>
  );
}
