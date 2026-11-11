import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useApplyTheme } from "../theme/useApplyTheme";
import { installMatchMedia } from "../theme/matchMediaMock";
import { useThemeStore } from "../theme/themeStore";
import { installMemoryStorage } from "../ui/memoryStorage";
import { ThemeSwitcher } from "./ThemeSwitcher";

function Harness() {
  useApplyTheme();
  return <ThemeSwitcher />;
}

describe("ThemeSwitcher", () => {
  beforeEach(() => {
    installMemoryStorage();
    installMatchMedia(false);
    useThemeStore.setState(useThemeStore.getInitialState(), true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders three radios with System checked", () => {
    render(<Harness />);
    expect(screen.getAllByRole("radio")).toHaveLength(3);
    expect(screen.getByRole("radio", { name: "System" })).toBeChecked();
  });

  it("choosing Dark checks it and sets data-theme", async () => {
    render(<Harness />);
    await userEvent.click(screen.getByRole("radio", { name: "Dark" }));

    expect(screen.getByRole("radio", { name: "Dark" })).toBeChecked();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
