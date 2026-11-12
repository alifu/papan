import { dismissToast, useToastStore } from "./toastStore";

/** Bottom-right toast stack. Mount once in the app shell. */
export function ToastHost() {
  const toasts = useToastStore((state) => state.toasts);
  return (
    <div className="toast-host">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`toast toast-${toast.kind}`}
          role={toast.kind === "error" ? "alert" : "status"}
        >
          <span className="toast-message">{toast.message}</span>
          <button
            type="button"
            className="toast-close"
            aria-label="Dismiss"
            onClick={() => dismissToast(toast.id)}
          >
            {"×"}
          </button>
        </div>
      ))}
    </div>
  );
}
