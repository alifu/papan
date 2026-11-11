export function isMac(): boolean {
  return navigator.platform.startsWith("Mac");
}

/** "N" becomes "⌘N" on macOS and "Ctrl+N" elsewhere. */
export function shortcutLabel(key: string): string {
  return isMac() ? `⌘${key}` : `Ctrl+${key}`;
}
