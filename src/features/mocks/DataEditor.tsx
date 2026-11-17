import type { DataBucket } from "../../ipc/bindings";
import { Button } from "../../ui/Button";
import { MAX_BUCKETS, newBucket, removeAt, replaceAt } from "./mockModel";

export interface DataEditorProps {
  readonly buckets: readonly DataBucket[];
  /** Ids of buckets that a data route serves; those cannot be removed. */
  readonly inUse: ReadonlySet<string>;
  readonly onChange: (buckets: DataBucket[]) => void;
}

function jsonProblem(text: string): string | null {
  try {
    JSON.parse(text);
    return null;
  } catch (err) {
    return err instanceof Error ? err.message : "Not valid JSON.";
  }
}

/** Named JSON the mock serves from data routes and templates (`{{data 'name'}}`). */
export function DataEditor({ buckets, inUse, onChange }: DataEditorProps) {
  const set = (index: number, patch: Partial<DataBucket>) =>
    onChange(replaceAt(buckets, index, { ...buckets[index]!, ...patch }));
  return (
    <div className="mock-data">
      <p className="empty-text">
        Data buckets live in memory while the mock runs and start again from
        these values. Data routes change them; templates read them with{" "}
        {"{{data 'name'}}"}.
      </p>
      {buckets.map((bucket, index) => {
        const n = index + 1;
        const text = bucket.value ?? "[]";
        const problem = jsonProblem(text);
        return (
          <fieldset key={bucket.id} className="mock-bucket">
            <legend>Bucket {n}</legend>
            <div className="mock-row">
              <label>
                Name
                <input
                  aria-label={`Bucket ${n} name`}
                  value={bucket.name}
                  onChange={(e) => set(index, { name: e.target.value })}
                />
              </label>
              <Button
                icon="close"
                aria-label={`Remove bucket ${n}`}
                disabled={inUse.has(bucket.id)}
                title={
                  inUse.has(bucket.id)
                    ? "A data route uses this bucket."
                    : undefined
                }
                onClick={() => onChange(removeAt(buckets, index))}
              />
            </div>
            <textarea
              aria-label={`Bucket ${n} JSON`}
              aria-invalid={problem !== null}
              value={text}
              rows={8}
              spellCheck={false}
              onChange={(e) => set(index, { value: e.target.value })}
            />
            {problem ? (
              <small role="alert">Not valid JSON: {problem}</small>
            ) : null}
          </fieldset>
        );
      })}
      <Button
        icon="plus"
        label="Add bucket"
        disabled={buckets.length >= MAX_BUCKETS}
        onClick={() =>
          onChange([...buckets, newBucket(`bucket${buckets.length + 1}`)])
        }
      />
    </div>
  );
}
