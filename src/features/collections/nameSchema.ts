import { z } from "zod";

export const NAME_MAX_LENGTH = 100;

// eslint-disable-next-line no-control-regex
const CONTROL_CHARACTERS = /[\u0000-\u001F\u007F]/;

/** Mirror of the backend name rule: trimmed, 1-100 chars, no control characters. */
export const nameSchema = z
  .string()
  .trim()
  .min(1, "Name is required.")
  .max(NAME_MAX_LENGTH, `Name must be ${NAME_MAX_LENGTH} characters or fewer.`)
  .refine((value) => !CONTROL_CHARACTERS.test(value), "Name cannot contain control characters.");

export type NameCheck =
  | { readonly ok: true; readonly name: string }
  | { readonly ok: false; readonly message: string };

export function checkName(input: string): NameCheck {
  const result = nameSchema.safeParse(input);
  return result.success
    ? { ok: true, name: result.data }
    : { ok: false, message: result.error.issues[0]?.message ?? "Name is not valid." };
}
