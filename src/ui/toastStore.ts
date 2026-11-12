import { create } from "zustand";
import { toUserMessage } from "../ipc/errors";

export const TOAST_AUTO_DISMISS_MS = 6000;
export const TOAST_MAX_VISIBLE = 3;

export type ToastKind = "error" | "info";

export interface ToastInput {
  readonly kind: ToastKind;
  readonly message: string;
}

export interface Toast extends ToastInput {
  readonly id: number;
}

interface ToastState {
  readonly toasts: readonly Toast[];
}

let nextToastId = 1;

export const useToastStore = create<ToastState>()(() => ({ toasts: [] }));

/** Removes a toast; unknown ids are ignored. */
export function dismissToast(id: number): void {
  useToastStore.setState((state) => ({
    toasts: state.toasts.filter((toast) => toast.id !== id),
  }));
}

/** Adds a toast (oldest dropped beyond the cap) and returns its id. */
export function showToast(input: ToastInput): number {
  const id = nextToastId++;
  useToastStore.setState((state) => ({
    toasts: [...state.toasts, { ...input, id }].slice(-TOAST_MAX_VISIBLE),
  }));
  setTimeout(() => dismissToast(id), TOAST_AUTO_DISMISS_MS);
  return id;
}

/** Shows an error toast with the user-safe text for any thrown value. */
export function reportError(err: unknown): number {
  return showToast({ kind: "error", message: toUserMessage(err) });
}
