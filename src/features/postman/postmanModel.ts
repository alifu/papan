import type {
  ImportOptions,
  PostmanFileSummary,
  PostmanPreview,
  ReportItem,
  ReportLevel,
} from "../../ipc/bindings";

export type ReportFilter = ReportLevel | "all";

export const REPORT_LEVELS: readonly ReportLevel[] = ["skipped", "warning", "info"];

export interface ReportGroup {
  readonly file: string;
  readonly items: readonly ReportItem[];
}

export type ImportTargetChoice = "new_collection" | "collection";

/** The user's choices on the preview step; turned into `ImportOptions` on commit. */
export interface ImportChoices {
  readonly target: ImportTargetChoice;
  readonly collectionId: string | null;
  readonly secretNames: readonly string[];
  readonly storeSecretValues: boolean;
  readonly importScripts: boolean;
  readonly globalsClash: ImportOptions["globals_clash"];
}

export const isCollectionFile = (file: PostmanFileSummary): boolean =>
  file.kind === "collection_v20" || file.kind === "collection_v21";

/** Environment and globals files need an open collection to land in. */
export function needsCollection(preview: PostmanPreview): boolean {
  return !preview.files.some(isCollectionFile);
}

export const hasGlobals = (preview: PostmanPreview): boolean => preview.files.some((f) => f.kind === "globals");

/** Sorted, de-duplicated secret candidates across all files. */
export function secretCandidates(preview: PostmanPreview): readonly string[] {
  return [...new Set(preview.files.flatMap((f) => f.secret_candidates))].sort();
}

export function defaultChoices(preview: PostmanPreview, openCollectionIds: readonly string[]): ImportChoices {
  const forced = needsCollection(preview);
  return {
    target: forced ? "collection" : "new_collection",
    collectionId: openCollectionIds[0] ?? null,
    secretNames: secretCandidates(preview),
    storeSecretValues: false,
    importScripts: true,
    globalsClash: "keep",
  };
}

/** Why the Import button is disabled, or null when the choices can be committed. */
export function importBlocker(preview: PostmanPreview, choices: ImportChoices): string | null {
  if (preview.files.length === 0) return "Nothing to import.";
  if (needsCollection(preview) && choices.target !== "collection") return "Environments and globals need an open collection.";
  if (choices.target === "collection" && choices.collectionId === null) return "Open a collection first.";
  return null;
}

export function toImportOptions(choices: ImportChoices): ImportOptions {
  const target: ImportOptions["target"] =
    choices.target === "collection" && choices.collectionId !== null
      ? { type: "collection", collection_id: choices.collectionId }
      : { type: "new_collection" };
  return {
    target,
    secret_names: [...choices.secretNames],
    store_secret_values: choices.storeSecretValues,
    import_scripts: choices.importScripts,
    globals_clash: choices.globalsClash,
  };
}

export function toggleName(names: readonly string[], name: string): readonly string[] {
  return names.includes(name) ? names.filter((n) => n !== name) : [...names, name];
}

export function reportCounts(report: readonly ReportItem[]): Readonly<Record<ReportLevel, number>> {
  const counts: Record<ReportLevel, number> = { skipped: 0, warning: 0, info: 0 };
  for (const item of report) counts[item.level] += 1;
  return counts;
}

export function filterReport(report: readonly ReportItem[], filter: ReportFilter): readonly ReportItem[] {
  return filter === "all" ? report : report.filter((item) => item.level === filter);
}

/** Groups by file in first-seen order; items keep their order. */
export function groupReport(report: readonly ReportItem[]): readonly ReportGroup[] {
  const files: string[] = [];
  for (const item of report) if (!files.includes(item.file)) files.push(item.file);
  return files.map((file) => ({ file, items: report.filter((item) => item.file === file) }));
}

export function reportToText(report: readonly ReportItem[]): string {
  return groupReport(report)
    .map(({ file, items }) =>
      [file, ...items.map((i) => `  [${i.level}] ${i.path ? `${i.path}: ` : ""}${i.message}`)].join("\n"),
    )
    .join("\n");
}
