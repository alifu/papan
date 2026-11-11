import { create } from "zustand";
import { z } from "zod";
import { readStored, writeStored } from "../ui/safeStorage";

export type NavSection = "collections" | "scratch" | "mocks" | "journeys";
export const NAV_SECTIONS: readonly NavSection[] = [
  "collections",
  "scratch",
  "mocks",
  "journeys",
];
export const SIDEBAR_MIN_WIDTH = 180;
export const SIDEBAR_MAX_WIDTH = 480;
export const SIDEBAR_DEFAULT_WIDTH = 260;
export const SIDEBAR_KEY_STEP = 16;
export const LAYOUT_STORAGE_KEY = "papan.layout";

export interface LayoutSnapshot {
  readonly sidebarWidth: number;
  readonly sidebarCollapsed: boolean;
  readonly activeSection: NavSection;
}

export interface LayoutState extends LayoutSnapshot {
  setSidebarWidth: (width: number) => void;
  toggleSidebar: () => void;
  setActiveSection: (section: NavSection) => void;
}

const DEFAULT_LAYOUT: LayoutSnapshot = {
  sidebarWidth: SIDEBAR_DEFAULT_WIDTH,
  sidebarCollapsed: false,
  activeSection: "collections",
};

const layoutSchema = z.object({
  sidebarWidth: z.number().finite(),
  sidebarCollapsed: z.boolean(),
  activeSection: z.enum(["collections", "scratch", "mocks", "journeys"]),
});

export function clampSidebarWidth(width: number): number {
  if (Number.isNaN(width)) return SIDEBAR_DEFAULT_WIDTH;
  return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)));
}

export function loadLayout(): LayoutSnapshot {
  const raw = readStored(LAYOUT_STORAGE_KEY);
  if (raw === null) return DEFAULT_LAYOUT;
  try {
    const parsed = layoutSchema.safeParse(JSON.parse(raw));
    if (!parsed.success) return DEFAULT_LAYOUT;
    return { ...parsed.data, sidebarWidth: clampSidebarWidth(parsed.data.sidebarWidth) };
  } catch {
    return DEFAULT_LAYOUT;
  }
}

function snapshotOf(state: LayoutSnapshot): LayoutSnapshot {
  const { sidebarWidth, sidebarCollapsed, activeSection } = state;
  return { sidebarWidth, sidebarCollapsed, activeSection };
}

export const useLayoutStore = create<LayoutState>()((set, get) => {
  const update = (patch: Partial<LayoutSnapshot>): void => {
    const next = { ...snapshotOf(get()), ...patch };
    writeStored(LAYOUT_STORAGE_KEY, JSON.stringify(next));
    set(next);
  };
  return {
    ...loadLayout(),
    setSidebarWidth: (width) => update({ sidebarWidth: clampSidebarWidth(width) }),
    toggleSidebar: () => update({ sidebarCollapsed: !get().sidebarCollapsed }),
    setActiveSection: (activeSection) => update({ activeSection }),
  };
});
