import type { ReactElement } from "react";
import type { IconName } from "../names";

const BLUE = "#2F6FD0";
const GREEN = "#1E8E3E";
const RED = "#D93025";
const ORANGE = "#D2691E";
const PURPLE = "#7E57C2";
const TEAL = "#00838F";
const PINK = "#C2185B";
const GREY = "#7A7A7A";
const LIGHT_BLUE = "#A6CAF0";
const WHITE = "#FFFFFF";

const HEX_POINTS = "8,2.5 13,5.25 13,10.75 8,13.5 3,10.75 3,5.25";

export const PROTOCOL_GLYPHS = {
  "request-http": (
    <g fill="none" strokeWidth="1" stroke={BLUE}>
      <circle cx="8" cy="8" r="6.5" fill={LIGHT_BLUE} />
      <ellipse cx="8" cy="8" rx="3" ry="6.5" />
      <path d="M1.5 8 H14.5" />
    </g>
  ),
  "request-graphql": (
    <g strokeWidth="1" stroke={PINK}>
      <polygon points={HEX_POINTS} fill={WHITE} />
      <g fill={PINK}>
        <circle cx="8" cy="2.5" r="1.5" />
        <circle cx="13" cy="5.25" r="1.5" />
        <circle cx="13" cy="10.75" r="1.5" />
        <circle cx="8" cy="13.5" r="1.5" />
        <circle cx="3" cy="10.75" r="1.5" />
        <circle cx="3" cy="5.25" r="1.5" />
      </g>
    </g>
  ),
  "request-grpc": (
    <g strokeWidth="1" stroke={TEAL}>
      <rect x="2" y="2" width="12" height="3" fill={LIGHT_BLUE} />
      <rect x="2" y="11" width="12" height="3" fill={LIGHT_BLUE} />
      <path d="M6 6.5 L8 8.5 L10 6.5" fill="none" strokeWidth="1.5" strokeLinejoin="round" />
      <path d="M6 9.5 L8 11.5 L10 9.5" fill="none" strokeWidth="1.5" strokeLinejoin="round" />
    </g>
  ),
  "request-soap": (
    <g strokeWidth="1" stroke={PURPLE} strokeLinejoin="round">
      <rect x="1.5" y="3.5" width="13" height="9" fill={WHITE} />
      <path d="M1.5 3.5 L8 9 L14.5 3.5" fill="none" />
      <path d="M6.5 4.5 L5 6 L6.5 7.5" fill="none" />
      <path d="M9.5 4.5 L11 6 L9.5 7.5" fill="none" />
    </g>
  ),
  "request-ws": (
    <g fill="none" strokeWidth="1.5" stroke={ORANGE}>
      <path d="M2 5.5 H11" />
      <path d="M5 10.5 H14" />
      <polygon points="14,5.5 10.5,3 10.5,8" fill={ORANGE} strokeWidth="1" />
      <polygon points="2,10.5 5.5,8 5.5,13" fill={ORANGE} strokeWidth="1" />
    </g>
  ),
  "webhook": (
    <g fill="none" strokeWidth="1.5" stroke={RED}>
      <path d="M4 5 V10 A3 3 0 0 0 10 10 V5" />
      <path d="M7 1.5 V7" />
      <polygon points="4.5,5 9.5,5 7,8" fill={RED} strokeWidth="1" />
    </g>
  ),
  "environment": (
    <g fill="none" strokeWidth="1.5" stroke={GREEN}>
      <path d="M5.5 2.5 Q3 2.5 3 5 V7 Q3 8 2 8 Q3 8 3 9 V11 Q3 13.5 5.5 13.5" />
      <path d="M10.5 2.5 Q13 2.5 13 5 V7 Q13 8 14 8 Q13 8 13 9 V11 Q13 13.5 10.5 13.5" />
      <circle cx="8" cy="8" r="1.5" fill={GREEN} stroke="none" />
    </g>
  ),
  "git": (
    <g fill={WHITE} strokeWidth="1.5" stroke={ORANGE}>
      <path d="M4 5 V11" fill="none" />
      <path d="M4 10 Q4 8 12 8" fill="none" />
      <circle cx="4" cy="3" r="2" />
      <circle cx="4" cy="13" r="2" />
      <circle cx="12" cy="8" r="2" />
    </g>
  ),
  "mock": (
    <g strokeWidth="1" stroke={GREY}>
      <rect x="2.5" y="2.5" width="11" height="11" rx="1" fill={WHITE} />
      <path d="M4.5 6 H9.5" fill="none" />
      <path d="M4.5 9 H9.5" fill="none" />
      <circle cx="11" cy="6" r="1.5" fill={GREEN} stroke={GREEN} />
    </g>
  ),
  "journey": (
    <g strokeWidth="1.5" stroke={PURPLE}>
      <path d="M3 5 V8 H10" fill="none" />
      <path d="M12 10 V13 H5" fill="none" />
      <circle cx="3" cy="3" r="2" fill={WHITE} />
      <circle cx="12" cy="8" r="2" fill={WHITE} />
      <circle cx="3" cy="13" r="2" fill={WHITE} />
    </g>
  ),
} satisfies Partial<Record<IconName, ReactElement>>;
