import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SearchBar } from "./SearchBar";

const setup = (over: Partial<Parameters<typeof SearchBar>[0]> = {}) => {
  const props = {
    query: "a",
    onQueryChange: vi.fn(),
    matchCount: 3,
    current: 2,
    onCurrentChange: vi.fn(),
    ...over,
  };
  render(<SearchBar {...props} />);
  return props;
};

describe("SearchBar", () => {
  it("shows the match position", () => {
    setup();
    expect(screen.getByText("3 of 3")).toBeInTheDocument();
  });
  it("shows no matches", () => {
    setup({ matchCount: 0, current: 0 });
    expect(screen.getByText("No matches")).toBeInTheDocument();
  });
  it("reports typed text", async () => {
    const props = setup({ query: "" });
    await userEvent.type(screen.getByRole("searchbox"), "x");
    expect(props.onQueryChange).toHaveBeenCalledWith("x");
  });
  it("wraps with Enter and goes back with Shift+Enter", async () => {
    const props = setup();
    await userEvent.type(screen.getByRole("searchbox"), "{Enter}");
    expect(props.onCurrentChange).toHaveBeenLastCalledWith(0);
    await userEvent.type(screen.getByRole("searchbox"), "{Shift>}{Enter}{/Shift}");
    expect(props.onCurrentChange).toHaveBeenLastCalledWith(1);
  });
  it("steps with the buttons and marks capped counts", async () => {
    const props = setup({ capped: true, current: 0 });
    expect(screen.getByText("1 of 3+")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(props.onCurrentChange).toHaveBeenCalledWith(2);
  });
});
