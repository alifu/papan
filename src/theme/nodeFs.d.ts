// Minimal typing for the one Node API the contrast test needs (no @types/node in the project).
declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}
