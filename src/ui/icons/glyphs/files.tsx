import type { ReactElement } from "react";
import type { IconName } from "../names";

const EDGE = "#B07A00";
const FILL = "#FFD75E";
const FILL_LIGHT = "#FFE9A0";
const GLOSS = "#FFF3C4";

const BACK_PANEL = "M1.5 3.5 H6 L7.5 5 H14.5 V13.5 H1.5 Z";

export const FILE_GLYPHS = {
  "folder": (
    <g strokeWidth="1" stroke={EDGE} strokeLinejoin="round">
      <path d={BACK_PANEL} fill={FILL} />
      <rect x="1.5" y="6.5" width="13" height="7" fill={FILL} />
      <path d="M2.5 7.5 H13.5" stroke={GLOSS} fill="none" />
    </g>
  ),
  "folder-open": (
    <g strokeWidth="1" stroke={EDGE} strokeLinejoin="round">
      <path d="M1.5 3.5 H6 L7.5 5 H14.5 V8 H1.5 Z" fill={FILL} />
      <path d="M1.5 13.5 L3.5 7.5 H15 L13 13.5 Z" fill={FILL_LIGHT} />
    </g>
  ),
} satisfies Partial<Record<IconName, ReactElement>>;
