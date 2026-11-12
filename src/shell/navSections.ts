import type { IconName } from "../ui/icons";
import type { NavSection } from "./layoutStore";

export interface NavSectionMeta {
  readonly label: string;
  readonly icon: IconName;
  readonly emptyText: string;
}

export const NAV_SECTION_META: Readonly<Record<NavSection, NavSectionMeta>> = {
  collections: { label: "Collections", icon: "folder", emptyText: "No collections yet." },
  scratch: { label: "Scratch", icon: "request-http", emptyText: "No scratch requests yet." },
  mocks: { label: "Mocks", icon: "mock", emptyText: "No mock servers yet." },
  history: { label: "History", icon: "history", emptyText: "No requests sent yet." },
  journeys: { label: "Journeys", icon: "journey", emptyText: "No journeys yet." },
};
