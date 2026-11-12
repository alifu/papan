import type { ReactElement } from "react";
import type { IconName } from "../names";

const GREEN = "#1E8E3E";
const RED = "#D93025";
const GREY = "#7A7A7A";
const LIGHT_GREY = "#D9D9D9";
const LIGHT_BLUE = "#A6CAF0";
const WHITE = "#FFFFFF";

const COG_ANGLES = [0, 45, 90, 135, 180, 225, 270, 315];

const COG_TEETH = COG_ANGLES.map((angle) => (
  <rect
    key={angle}
    x="7.25"
    y="1.25"
    width="1.5"
    height="2.5"
    fill={GREY}
    transform={`rotate(${angle} 8 8)`}
  />
));

export const ACTION_GLYPHS = {
  "play": (
    <polygon
      points="5,3 13,8 5,13"
      fill={GREEN}
      stroke={GREEN}
      strokeWidth="1"
      strokeLinejoin="round"
    />
  ),
  "stop": (
    <rect x="3.5" y="3.5" width="9" height="9" rx="1" fill={RED} stroke={RED} strokeWidth="1" />
  ),
  "plus": (
    <path d="M8 3 V13 M3 8 H13" fill="none" stroke={GREEN} strokeWidth="1.5" strokeLinecap="square" />
  ),
  "settings": (
    <g>
      {COG_TEETH}
      <circle cx="8" cy="8" r="4.5" fill={LIGHT_GREY} stroke={GREY} strokeWidth="1.5" />
      <circle cx="8" cy="8" r="1.5" fill={WHITE} stroke={GREY} strokeWidth="1" />
    </g>
  ),
  "history": (
    <g>
      <circle cx="8" cy="8" r="5.5" fill={WHITE} stroke={GREY} strokeWidth="1.5" />
      <path d="M8 4.5 V8 L10.5 9.5" fill="none" stroke={GREY} strokeWidth="1.25" strokeLinecap="round" />
    </g>
  ),
  "search": (
    <g>
      <path d="M10 10 L14 14" fill="none" stroke={GREY} strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="6.5" cy="6.5" r="4.5" fill={LIGHT_BLUE} stroke={GREY} strokeWidth="1.5" />
    </g>
  ),
} satisfies Partial<Record<IconName, ReactElement>>;
