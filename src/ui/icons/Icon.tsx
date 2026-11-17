import { GLYPHS } from "./glyphs";
import type { IconName } from "./names";

export interface IconProps {
  readonly name: IconName;
  readonly size?: 16 | 24;
  readonly title?: string;
  readonly className?: string;
}

const DEFAULT_SIZE = 16;
const VIEW_BOX = "0 0 16 16";

export function Icon({ name, size = DEFAULT_SIZE, title, className }: IconProps) {
  const isLabelled = title !== undefined;
  const classes = ["icon", className].filter(Boolean).join(" ");

  return (
    <svg
      viewBox={VIEW_BOX}
      width={size}
      height={size}
      className={classes}
      focusable="false"
      role={isLabelled ? "img" : undefined}
      aria-hidden={isLabelled ? undefined : "true"}
    >
      {isLabelled && <title>{title}</title>}
      {GLYPHS[name]}
    </svg>
  );
}
