import { create } from "zustand";
import { z } from "zod";
import { readStored, writeStored } from "../ui/safeStorage";

export type ThemeMode = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_MODES: readonly ThemeMode[] = ["system", "light", "dark"];
export const THEME_STORAGE_KEY = "papan.theme";
export const DARK_SCHEME_QUERY = "(prefers-color-scheme: dark)";

const themeModeSchema = z.enum(["system", "light", "dark"]);

export interface ThemeState {
  readonly mode: ThemeMode;
  readonly systemTheme: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
  setSystemTheme: (theme: ResolvedTheme) => void;
}

export function loadThemeMode(): ThemeMode {
  const parsed = themeModeSchema.safeParse(readStored(THEME_STORAGE_KEY));
  return parsed.success ? parsed.data : "system";
}

export function readSystemTheme(): ResolvedTheme {
  return window.matchMedia?.(DARK_SCHEME_QUERY).matches ? "dark" : "light";
}

export function resolveTheme(
  mode: ThemeMode,
  systemTheme: ResolvedTheme,
): ResolvedTheme {
  return mode === "system" ? systemTheme : mode;
}

export const useThemeStore = create<ThemeState>()((set) => ({
  mode: loadThemeMode(),
  systemTheme: readSystemTheme(),
  setMode: (mode) => {
    writeStored(THEME_STORAGE_KEY, mode);
    set({ mode });
  },
  setSystemTheme: (systemTheme) => set({ systemTheme }),
}));
