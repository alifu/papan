import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ImportReport } from "./ImportReport";

const copyText = vi.fn();
vi.mock("../response/clipboard", () => ({ copyText: (t: string) => copyText(t) }));

const report = [
  { level: "warning", file: "a.json", path: "$.x", message: "careful" },
  { level: "info", file: "b.json", path: "", message: "fyi" },
] as const;

describe("ImportReport", () => {
  beforeEach(() => copyText.mockReset());

  it("says so when empty", () => {
    render(<ImportReport report={[]} />);
    expect(screen.getByText("Nothing to report.")).toBeVisible();
  });

  it("filters by level", async () => {
    render(<ImportReport report={report} />);
    expect(screen.getByText("careful")).toBeVisible();
    await userEvent.selectOptions(screen.getByLabelText("Report level"), "info");
    expect(screen.queryByText("careful")).toBeNull();
    expect(screen.getByText("fyi")).toBeVisible();
  });

  it("copies the filtered report as text", async () => {
    render(<ImportReport report={report} truncated={4} />);
    expect(screen.getByText(/4 more items/)).toBeVisible();
    await userEvent.selectOptions(screen.getByLabelText("Report level"), "warning");
    await userEvent.click(screen.getByRole("button", { name: "Copy report" }));
    expect(copyText).toHaveBeenCalledWith("a.json\n  [warning] $.x: careful");
  });
});
