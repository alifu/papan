import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { StoredCookie } from "../../ipc/bindings";
import { useToastStore } from "../../ui/toastStore";
import * as api from "./api";
import { parseExpiry } from "./CookieForm";
import { CookieManager } from "./CookieManager";

vi.mock("./api");

const location = { scope: "scratch" as const, id: "s1" };
const cookie = (over: Partial<StoredCookie>): StoredCookie => ({
  name: "sid", value: "abc", domain: "a.test", path: "/", expires_at_ms: null, http_only: true, secure: false, ...over,
});
const jar = [cookie({}), cookie({ name: "theme", domain: "b.test" })];

beforeEach(() => {
  vi.resetAllMocks();
  useToastStore.setState({ toasts: [] });
  vi.mocked(api.listCookies).mockResolvedValue(jar);
});

describe("parseExpiry", () => {
  it("handles empty, UTC, offset and bad text", () => {
    expect(parseExpiry("")).toBeNull();
    expect(parseExpiry("2030-01-31 12:00")).toBe(Date.parse("2030-01-31T12:00Z"));
    expect(parseExpiry("2030-01-31T12:00:00+02:00")).toBe(Date.parse("2030-01-31T10:00Z"));
    expect(parseExpiry("soon")).toBeUndefined();
  });
});

describe("CookieManager", () => {
  it("groups by domain and filters", async () => {
    render(<CookieManager location={location} onClose={vi.fn()} />);
    expect(await screen.findByRole("region", { name: "Cookies for a.test" })).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Filter cookies"), "theme");
    expect(screen.queryByRole("region", { name: "Cookies for a.test" })).toBeNull();
    await userEvent.clear(screen.getByLabelText("Filter cookies"));
    await userEvent.type(screen.getByLabelText("Filter cookies"), "zzz");
    expect(screen.getByText("No cookies.")).toBeInTheDocument();
  });

  it("deletes one cookie and a whole domain", async () => {
    vi.mocked(api.deleteCookies).mockResolvedValue([]);
    render(<CookieManager location={location} onClose={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Delete cookie sid" }));
    expect(api.deleteCookies).toHaveBeenCalledWith(location, { domain: "a.test", path: "/", name: "sid" });
    expect(await screen.findByText("No cookies.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add cookie…" }));
    await userEvent.click(screen.getAllByRole("button", { name: "Cancel" })[1]!);
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
  });

  it("deletes a domain", async () => {
    vi.mocked(api.deleteCookies).mockResolvedValue([jar[1]!]);
    render(<CookieManager location={location} onClose={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Delete all cookies of a.test" }));
    expect(api.deleteCookies).toHaveBeenCalledWith(location, { domain: "a.test", path: null, name: null });
  });

  it("adds and edits a cookie", async () => {
    vi.mocked(api.setCookie).mockResolvedValue(jar);
    render(<CookieManager location={location} onClose={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Add cookie…" }));
    const form = screen.getAllByRole("dialog")[1]!;
    await userEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(within(form).getByRole("alert")).toHaveTextContent("Name and domain are required.");
    await userEvent.type(within(form).getByLabelText("Name"), "n");
    await userEvent.type(within(form).getByLabelText("Domain"), "d.test");
    await userEvent.type(within(form).getByLabelText(/Expires/), "bad");
    await userEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(within(form).getByRole("alert")).toHaveTextContent(/Expiry must be/);
    await userEvent.clear(within(form).getByLabelText(/Expires/));
    await userEvent.click(within(form).getByLabelText("Secure"));
    await userEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(api.setCookie).toHaveBeenCalledWith(location, expect.objectContaining({ name: "n", domain: "d.test", path: "/", secure: true, expires_at_ms: null }));
    await vi.waitFor(() => expect(screen.getAllByRole("dialog")).toHaveLength(1));
    await userEvent.click(screen.getByRole("button", { name: "Edit cookie sid" }));
    expect(screen.getByDisplayValue("abc")).toBeInTheDocument();
  });

  it("clears all after confirming", async () => {
    vi.mocked(api.clearCookies).mockResolvedValue(null);
    render(<CookieManager location={location} onClose={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Clear all" }));
    await userEvent.click(screen.getAllByRole("button", { name: "Clear all" })[1]!);
    expect(api.clearCookies).toHaveBeenCalledWith(location);
    expect(await screen.findByText("No cookies.")).toBeInTheDocument();
  });

  it("reports a failed load", async () => {
    vi.mocked(api.listCookies).mockRejectedValue({ code: "internal", message: "nope" });
    render(<CookieManager location={location} onClose={vi.fn()} />);
    expect(await screen.findByText("No cookies.")).toBeInTheDocument();
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("toasts a failed write and keeps the list", async () => {
    vi.mocked(api.deleteCookies).mockRejectedValue({ code: "internal", message: "not available yet" });
    render(<CookieManager location={location} onClose={vi.fn()} />);
    await userEvent.click(await screen.findByRole("button", { name: "Delete cookie sid" }));
    await vi.waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(screen.getByRole("region", { name: "Cookies for a.test" })).toBeInTheDocument();
  });
});
