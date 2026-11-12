import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS } from "./content";
import { SettingsEditor } from "./SettingsEditor";

describe("SettingsEditor", () => {
  it("changes the HTTP version", async () => {
    const onChange = vi.fn();
    render(<SettingsEditor settings={DEFAULT_SETTINGS} onChange={onChange} />);
    await userEvent.selectOptions(screen.getByLabelText("HTTP version"), "http2");
    expect(onChange).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, http_version: "http2" });
  });

  it("rejects an invalid timeout", async () => {
    render(<SettingsEditor settings={DEFAULT_SETTINGS} onChange={vi.fn()} />);
    await userEvent.type(screen.getByLabelText(/Timeout/), "x");
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });
});
