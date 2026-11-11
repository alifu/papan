import type { ButtonHTMLAttributes } from "react";
import { Icon, type IconName } from "./icons";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  readonly icon?: IconName;
  readonly label?: string;
  readonly pressed?: boolean;
}

/** Bevel button. Icon-only buttons must pass `aria-label`. */
export function Button({
  icon,
  label,
  pressed,
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  const classes = ["btn", className].filter(Boolean).join(" ");
  return (
    <button
      type={type}
      className={classes}
      aria-pressed={pressed}
      {...rest}
    >
      {icon ? <Icon name={icon} /> : null}
      {label ? <span className="btn-label">{label}</span> : null}
    </button>
  );
}
