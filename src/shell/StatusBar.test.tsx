import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "../ipc/bindings";
import { StatusBar } from "./StatusBar";

vi.mock("../ipc/bindings", () => ({ commands: { appInfo: vi.fn() } }));

const appInfo = vi.mocked(commands.appInfo);

describe("StatusBar", () => {
  beforeEach(() => {
    appInfo.mockReset();
  });

  it("shows Ready and the app name and version", async () => {
    appInfo.mockResolvedValue({ name: "Papan", version: "0.1.0" });
    render(<StatusBar />);

    expect(await screen.findByText("Papan 0.1.0")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
  });

  it("shows an AppError message in an alert", async () => {
    appInfo.mockRejectedValue({ code: "io", message: "Disk is on fire" });
    render(<StatusBar />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Disk is on fire");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows the generic message for a non-AppError rejection", async () => {
    appInfo.mockRejectedValue("boom");
    render(<StatusBar />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Something went wrong. Please try again.",
    );
  });

  it("ignores a result that arrives after unmount", async () => {
    let resolve: (v: { name: string; version: string }) => void = () => {};
    appInfo.mockReturnValue(new Promise((r) => (resolve = r)));
    const { unmount } = render(<StatusBar />);
    unmount();
    resolve({ name: "Papan", version: "9" });
    await Promise.resolve();
    expect(screen.queryByText("Papan 9")).toBeNull();
  });
});
