import { create } from "zustand";

export type ShellDialog = "network" | "cookies" | "import-curl" | "import-postman" | "export-postman" | "git" | "clone-git";

interface DialogsState {
  readonly open: ShellDialog | null;
  /** Collection a dialog was opened for (the row menu's Export to Postman), else null. */
  readonly collectionId: string | null;
  show: (dialog: ShellDialog, collectionId?: string) => void;
  close: () => void;
}

/** Which menu-opened dialog is showing; the menu, shortcuts and response pane all open them here. */
export const useDialogsStore = create<DialogsState>()((set) => ({
  open: null,
  collectionId: null,
  show: (dialog, collectionId) => set({ open: dialog, collectionId: collectionId ?? null }),
  close: () => set({ open: null, collectionId: null }),
}));
