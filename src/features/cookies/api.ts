import { commands } from "../../ipc/bindings";
import type { CookieFilter, CookieInput, RequestLocation, StoredCookie } from "../../ipc/bindings";

// Thin wrappers over the generated bindings; unit tests `vi.mock` this module.
export const listCookies = (location: RequestLocation): Promise<StoredCookie[]> => commands.listCookies(location);
export const setCookie = (location: RequestLocation, cookie: CookieInput): Promise<StoredCookie[]> =>
  commands.setCookie(location, cookie);
export const deleteCookies = (location: RequestLocation, filter: CookieFilter): Promise<StoredCookie[]> =>
  commands.deleteCookies(location, filter);
export const clearCookies = (location: RequestLocation): Promise<null> => commands.clearCookies(location);
