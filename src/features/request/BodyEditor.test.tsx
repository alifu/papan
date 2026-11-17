import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { scratchLocation } from "../locations";
import { BodyEditor } from "./BodyEditor";

vi.mock("./api");
vi.mock("../../ui/code/CodeEditor", () => import("../../ui/code/CodeEditor.mock"));

const loc = scratchLocation("s1");
const optionNames = () => screen.getAllByRole("option").map((o) => o.textContent);

describe("BodyEditor", () => {
  it("offers every body type by default", () => {
    render(<BodyEditor location={loc} body={{ type: "none" }} onChange={() => {}} />);
    expect(optionNames()).toEqual(["None", "JSON", "XML", "Text", "Form", "Multipart", "Binary"]);
  });

  it("offers only the allowed types", () => {
    render(<BodyEditor location={loc} body={{ type: "xml", text: "" }} allowedTypes={["xml"]} onChange={() => {}} />);
    expect(optionNames()).toEqual(["XML"]);
  });

  it("formats XML with the Format button", async () => {
    const onChange = vi.fn();
    render(<BodyEditor location={loc} body={{ type: "xml", text: "<a><b/></a>" }} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: "Format" }));
    expect(onChange).toHaveBeenCalledWith({ type: "xml", text: "<a>\n  <b/>\n</a>" });
  });
});
