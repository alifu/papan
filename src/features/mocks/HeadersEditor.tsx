import type { KeyValue } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { removeAt, replaceAt } from "./mockModel";

export const MAX_HEADERS = 100;

export interface HeadersEditorProps {
  readonly label: string;
  readonly headers: readonly KeyValue[];
  readonly onChange: (headers: KeyValue[]) => void;
}

/** Name/value rows with an enable switch. */
export function HeadersEditor({
  label,
  headers,
  onChange,
}: HeadersEditorProps) {
  const set = (index: number, patch: Partial<KeyValue>) =>
    onChange(replaceAt(headers, index, { ...headers[index]!, ...patch }));
  return (
    <div className="mock-headers" role="group" aria-label={label}>
      {headers.map((header, index) => (
        <div key={index} className="mock-row">
          <input
            type="checkbox"
            aria-label={`${label} ${index + 1} enabled`}
            checked={header.enabled}
            onChange={(e) => set(index, { enabled: e.target.checked })}
          />
          <input
            aria-label={`${label} name ${index + 1}`}
            value={header.name}
            placeholder="Name"
            onChange={(e) => set(index, { name: e.target.value })}
          />
          <input
            aria-label={`${label} value ${index + 1}`}
            value={header.value}
            placeholder="Value"
            onChange={(e) => set(index, { value: e.target.value })}
          />
          <Button
            icon="close"
            aria-label={`Remove ${label} ${index + 1}`}
            onClick={() => onChange(removeAt(headers, index))}
          />
        </div>
      ))}
      <Button
        icon="plus"
        label="Add header"
        aria-label={`Add ${label}`}
        disabled={headers.length >= MAX_HEADERS}
        onClick={() =>
          onChange([...headers, { name: "", value: "", enabled: true }])
        }
      />
    </div>
  );
}
