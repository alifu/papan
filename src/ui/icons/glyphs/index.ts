import type { ReactElement } from "react";
import type { IconName } from "../names";
import { ACTION_GLYPHS } from "./actions";
import { CHROME_GLYPHS } from "./chrome";
import { FILE_GLYPHS } from "./files";
import { PROTOCOL_GLYPHS } from "./protocols";

export const GLYPHS: Readonly<Record<IconName, ReactElement>> = {
  ...FILE_GLYPHS,
  ...PROTOCOL_GLYPHS,
  ...ACTION_GLYPHS,
  ...CHROME_GLYPHS,
};
