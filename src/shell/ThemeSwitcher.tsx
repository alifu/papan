import { THEME_MODES, useThemeStore, type ThemeMode } from "../theme/themeStore";
import { Button } from "../ui/Button";
import type { IconName } from "../ui/icons";

const MODE_META: Readonly<Record<ThemeMode, { label: string; icon: IconName }>> = {
  system: { label: "System", icon: "monitor" },
  light: { label: "Light", icon: "sun" },
  dark: { label: "Dark", icon: "moon" },
};

export function ThemeSwitcher() {
  const mode = useThemeStore((s) => s.mode);
  const setMode = useThemeStore((s) => s.setMode);

  return (
    <div role="radiogroup" aria-label="Theme" className="theme-switcher">
      {THEME_MODES.map((option) => (
        <Button
          key={option}
          role="radio"
          aria-checked={mode === option}
          icon={MODE_META[option].icon}
          aria-label={MODE_META[option].label}
          title={MODE_META[option].label}
          className={mode === option ? "btn-checked" : undefined}
          onClick={() => setMode(option)}
        />
      ))}
    </div>
  );
}
