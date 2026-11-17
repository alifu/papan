import { useState } from "react";
import { z } from "zod";
import type { RequestContent } from "../../ipc/bindings";
import { grpcSettingsOf, MAX_DEADLINE_MS } from "./grpcModel";

export interface DeadlineFieldProps {
  readonly content: RequestContent;
  readonly onChange: (update: (content: RequestContent) => RequestContent) => void;
}

const deadlineSchema = z.coerce.number().int().min(0).max(MAX_DEADLINE_MS);
export const DEADLINE_ERROR = `Deadline must be a whole number from 0 to ${MAX_DEADLINE_MS} ms.`;

/** The gRPC call deadline (sent as `grpc-timeout`); 0 means none. */
export function DeadlineField({ content, onChange }: DeadlineFieldProps) {
  const [text, setText] = useState(String(grpcSettingsOf(content).deadline_ms));
  const [error, setError] = useState<string | null>(null);
  const edit = (next: string) => {
    setText(next);
    const parsed = next.trim() === "" ? null : deadlineSchema.safeParse(next);
    if (!parsed?.success) {
      setError(DEADLINE_ERROR);
      return;
    }
    setError(null);
    onChange((c) => ({ ...c, grpc: { ...grpcSettingsOf(c), deadline_ms: parsed.data } }));
  };
  return (
    <div className="editor-form">
      <label>
        Deadline (ms, 0 = none)
        <input inputMode="numeric" aria-label="Deadline" aria-invalid={error !== null} value={text} onChange={(e) => edit(e.target.value)} />
      </label>
      {error ? <p role="alert" className="field-error">{error}</p> : null}
    </div>
  );
}
