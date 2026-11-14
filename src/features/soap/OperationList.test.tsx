import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { makeWsdl } from "./fixtures";
import { OperationList } from "./OperationList";

describe("OperationList", () => {
  it("shows service, port with version badge and operations, and reports a pick", async () => {
    const onSelect = vi.fn();
    render(<OperationList wsdl={makeWsdl()} selected={null} onSelect={onSelect} />);
    expect(screen.getByText("Calc")).toBeVisible();
    expect(screen.getByText("SOAP 1.2")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Sub" }));

    expect(onSelect).toHaveBeenCalledWith({ service: "Calc", port: "CalcSoap12", operation: "Sub" });
  });

  it("marks the selected operation and works from the keyboard", async () => {
    const onSelect = vi.fn();
    render(
      <OperationList
        wsdl={makeWsdl()}
        selected={{ service: "Calc", port: "CalcSoap12", operation: "Add" }}
        onSelect={onSelect}
      />,
    );
    expect(screen.getByRole("button", { name: "Add" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Sub" })).toHaveAttribute("aria-pressed", "false");

    await userEvent.tab();
    await userEvent.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("says so when there are no services", () => {
    render(<OperationList wsdl={makeWsdl({ services: [] })} selected={null} onSelect={vi.fn()} />);
    expect(screen.getByText("This WSDL has no services.")).toBeVisible();
  });
});
