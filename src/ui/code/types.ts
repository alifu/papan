/** Structural subset of the generated `EffectiveVariable` binding, so this module needs no IPC import. */
export interface EditorVariable {
  readonly name: string;
  readonly value: string;
  readonly source: "global" | "collection" | "environment" | "dynamic";
  readonly secret: boolean;
  readonly resolved: boolean;
}

export interface EditorRange {
  readonly from: number;
  readonly to: number;
}

/** Search highlights; `current` indexes into `ranges`. */
export interface EditorMarks {
  readonly ranges: readonly EditorRange[];
  readonly current: number | null;
}

export type EditorLanguage = "json" | "xml" | "text";
