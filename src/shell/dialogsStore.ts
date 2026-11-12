import { create } from "zustand";

export type ShellDialog = "network" | "cookies" | "import-curl";

interface DialogsState {
  readonly open: ShellDialog | null;
  show: (dialog: ShellDialog) => void;
  close: () => void;
}

/** Which menu-opened dialog is showing; the menu, shortcuts and response pane all open them here. */
export const useDialogsStore = create<DialogsState>()((set) => ({
  open: null,
  show: (dialog) => set({ open: dialog }),
  close: () => set({ open: null }),
}));
