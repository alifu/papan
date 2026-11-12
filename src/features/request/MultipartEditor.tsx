import type { Part, RequestLocation } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { reportError } from "../../ui/toastStore";
import { pickBodyFile } from "./api";

export interface MultipartEditorProps {
  readonly parts: readonly Part[];
  readonly onChange: (parts: readonly Part[]) => void;
  /** Where the request lives: picked files are granted to that location only. */
  readonly location: RequestLocation;
}

const baseName = (path: string): string => path.split(/[\\/]/).pop() ?? path;

export function MultipartEditor({ parts, onChange, location }: MultipartEditorProps) {
  const update = (index: number, change: Partial<Part>) =>
    onChange(parts.map((p, i) => (i === index ? { ...p, ...change } : p)));

  const choose = async (index: number) => {
    try {
      const picked = await pickBodyFile(location);
      if (picked) update(index, { file: picked.path });
    } catch (err) {
      reportError(err);
    }
  };

  return (
    <div className="multipart">
      {parts.map((part, index) => {
        const isFile = part.file !== null;
        return (
          <div className="multipart-row" key={index}>
            <input
              type="checkbox"
              aria-label={`Enable part ${index + 1}`}
              checked={part.enabled}
              onChange={(e) => update(index, { enabled: e.target.checked })}
            />
            <input
              aria-label={`Part name ${index + 1}`}
              spellCheck={false}
              value={part.name}
              onChange={(e) => update(index, { name: e.target.value })}
            />
            <select
              aria-label={`Part type ${index + 1}`}
              value={isFile ? "file" : "text"}
              onChange={(e) => update(index, e.target.value === "file" ? { file: "" } : { file: null })}
            >
              <option value="text">Text</option>
              <option value="file">File</option>
            </select>
            {isFile ? (
              <>
                <span title={part.file ?? undefined}>{part.file ? baseName(part.file) : "No file chosen"}</span>
                <Button label="Choose file…" aria-label={`Choose file ${index + 1}`} onClick={() => void choose(index)} />
              </>
            ) : (
              <input
                aria-label={`Part value ${index + 1}`}
                spellCheck={false}
                value={part.value}
                onChange={(e) => update(index, { value: e.target.value })}
              />
            )}
            <Button label="Delete" aria-label={`Delete part ${index + 1}`} onClick={() => onChange(parts.filter((_, i) => i !== index))} />
          </div>
        );
      })}
      <Button
        label="Add part"
        onClick={() => onChange([...parts, { name: "", value: "", file: null, enabled: true }])}
      />
    </div>
  );
}
