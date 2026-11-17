import { useEffect } from "react";
import {
  DARK_SCHEME_QUERY,
  resolveTheme,
  useThemeStore,
  type ResolvedTheme,
} from "./themeStore";

export function applyTheme(theme: ResolvedTheme): void {
  document.documentElement.dataset.theme = theme;
}

/** Keeps `<html data-theme>` in sync with the chosen mode and the OS setting. */
export function useApplyTheme(): void {
  const mode = useThemeStore((s) => s.mode);
  const systemTheme = useThemeStore((s) => s.systemTheme);
  const setSystemTheme = useThemeStore((s) => s.setSystemTheme);

  useEffect(() => {
    const query = window.matchMedia?.(DARK_SCHEME_QUERY);
    if (!query) return;
    setSystemTheme(query.matches ? "dark" : "light");
    const onChange = (event: { matches: boolean }) =>
      setSystemTheme(event.matches ? "dark" : "light");
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, [setSystemTheme]);

  const resolved = resolveTheme(mode, systemTheme);
  useEffect(() => applyTheme(resolved), [resolved]);
}
