import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { ToastHost } from "./Toast";
import { showToast, useToastStore } from "./toastStore";

describe("ToastHost", () => {
  beforeEach(() => useToastStore.setState({ toasts: [] }));

  it("renders errors as alerts and info as status", () => {
    render(<ToastHost />);
    act(() => {
      showToast({ kind: "error", message: "Failed" });
      showToast({ kind: "info", message: "Saved" });
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Failed");
    expect(screen.getByRole("status")).toHaveTextContent("Saved");
  });

  it("closes a toast with its close button", async () => {
    render(<ToastHost />);
    act(() => void showToast({ kind: "info", message: "Saved" }));
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("status")).toBeNull();
  });
});
