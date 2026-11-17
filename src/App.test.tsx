import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { installMatchMedia } from "./theme/matchMediaMock";
import { installMemoryStorage } from "./ui/memoryStorage";

vi.mock("./ipc/bindings", () => ({
  commands: {
    appInfo: vi.fn().mockResolvedValue({ name: "Papan", version: "0.1.0" }),
    listCollections: vi.fn().mockResolvedValue([]),
    listScratch: vi.fn().mockResolvedValue([]),
    listWebhooks: vi.fn().mockResolvedValue([]),
  },
  events: {
    collectionChanged: { listen: vi.fn().mockResolvedValue(() => {}) },
    webhookCaptured: { listen: vi.fn().mockResolvedValue(() => {}) },
    webhookStopped: { listen: vi.fn().mockResolvedValue(() => {}) },
    mockLogged: { listen: vi.fn().mockResolvedValue(() => {}) },
    journeyRunEvent: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

describe("App", () => {
  beforeEach(() => {
    installMemoryStorage();
    installMatchMedia(false);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders the application shell", async () => {
    render(<App />);

    expect(screen.getByRole("menubar")).toBeVisible();
    expect(screen.getByRole("toolbar", { name: "Main" })).toBeVisible();
    expect(
      screen.getByRole("navigation", { name: "Navigation" }),
    ).toBeVisible();
    expect(screen.getByRole("tab", { name: "Welcome" })).toBeVisible();
    expect(await screen.findByText("Papan 0.1.0")).toBeVisible();
  });
});
