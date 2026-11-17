import type { AppError, ErrorCode } from "./bindings";

export type { AppError, ErrorCode };

const GENERIC_MESSAGE = "Something went wrong. Please try again.";

/** Shown when the backend sends an error without a usable message. */
const FALLBACK_MESSAGES: Readonly<Record<ErrorCode, string>> = {
  invalid_input: "The input is not valid.",
  not_found: "The requested item could not be found.",
  io: "A file operation failed.",
  internal: GENERIC_MESSAGE,
  network: "The server could not be reached.",
  timeout: "The request timed out.",
  cancelled: "The request was cancelled.",
};

function isErrorCode(value: unknown): value is ErrorCode {
  return (
    typeof value === "string" &&
    Object.prototype.hasOwnProperty.call(FALLBACK_MESSAGES, value)
  );
}

/**
 * True when `value` is the error object a Rust command rejects with.
 * Anything else (an `Error`, a string from Tauri itself, ...) is not an `AppError`.
 */
export function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const { code, message, detail } = value as Record<string, unknown>;
  const hasValidDetail =
    detail === undefined || detail === null || typeof detail === "string";
  return isErrorCode(code) && typeof message === "string" && hasValidDetail;
}

/**
 * Text that is safe to show to the user for any thrown value.
 * `AppError.message` is written for users; `detail` is for logs and is never shown.
 * Errors that did not come from our backend get a generic message, because their
 * text is technical.
 */
export function toUserMessage(err: unknown): string {
  if (!isAppError(err)) {
    return GENERIC_MESSAGE;
  }
  const message = err.message.trim();
  return message === "" ? FALLBACK_MESSAGES[err.code] : message;
}
