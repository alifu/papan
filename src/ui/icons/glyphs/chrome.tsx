import type { ReactElement } from "react";
import type { IconName } from "../names";

const BLUE = "#2F6FD0";
const GREY = "#7A7A7A";
const GOLD = "#B07A00";
const FILL_GOLD = "#FFD75E";
const LIGHT_BLUE = "#A6CAF0";
const WHITE = "#FFFFFF";

const RAY_ANGLES = [0, 45, 90, 135, 180, 225, 270, 315];

const SUN_RAYS = RAY_ANGLES.map((angle) => (
  <line
    key={angle}
    x1="8"
    y1="1.5"
    x2="8"
    y2="3"
    stroke={GOLD}
    strokeWidth="1.5"
    strokeLinecap="round"
    transform={`rotate(${angle} 8 8)`}
  />
));

export const CHROME_GLYPHS = {
  "close": (
    <path
      d="M4 4 L12 12 M12 4 L4 12"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
    />
  ),
  "chevron-left": (
    <polyline
      points="10,3.5 5.5,8 10,12.5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  ),
  "chevron-right": (
    <polyline
      points="6,3.5 10.5,8 6,12.5"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  ),
  "chevron-down": (
    <polyline
      points="3.5,6 8,10.5 12.5,6"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  ),
  "sun": (
    <g>
      {SUN_RAYS}
      <circle cx="8" cy="8" r="3.5" fill={FILL_GOLD} stroke={GOLD} strokeWidth="1" />
    </g>
  ),
  "moon": (
    <path
      d="M9 1.5 A6.5 6.5 0 1 0 14.5 10 A5.2 5.2 0 0 1 9 1.5 Z"
      fill={LIGHT_BLUE}
      stroke={BLUE}
      strokeWidth="1"
      strokeLinejoin="round"
    />
  ),
  "monitor": (
    <g>
      <path d="M8 11 V13" fill="none" stroke={GREY} strokeWidth="1.5" />
      <path d="M5 13.5 H11" fill="none" stroke={GREY} strokeWidth="1.5" strokeLinecap="round" />
      <rect x="2" y="2.5" width="12" height="8.5" rx="1" fill={LIGHT_BLUE} stroke={BLUE} strokeWidth="1.5" />
      <rect x="3.5" y="4" width="9" height="5.5" fill={WHITE} />
    </g>
  ),
} satisfies Partial<Record<IconName, ReactElement>>;
