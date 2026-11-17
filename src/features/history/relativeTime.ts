const STEPS: readonly (readonly [number, string])[] = [
  [24 * 60 * 60, "d"],
  [60 * 60, "h"],
  [60, "m"],
];

/** "just now", "5m ago", "3h ago", "2d ago"; empty when the time is unknown. */
export function relativeTime(sentAtMs: number | null, nowMs: number): string {
  if (sentAtMs === null) return "";
  const seconds = Math.max(0, Math.floor((nowMs - sentAtMs) / 1000));
  const step = STEPS.find(([size]) => seconds >= size);
  return step ? `${Math.floor(seconds / step[0])}${step[1]} ago` : "just now";
}
