import { describe, expect, it } from "vitest";
import { formatXmlText } from "./xmlFormat";

describe("formatXmlText", () => {
  it("indents nested elements and keeps text elements on one line", () => {
    const input = '<?xml version="1.0"?><a><b>1</b><c><d/></c><!-- n --></a>';
    expect(formatXmlText(input)).toBe(
      ['<?xml version="1.0"?>', "<a>", "  <b>1</b>", "  <c>", "    <d/>", "  </c>", "  <!-- n -->", "</a>"].join("\n"),
    );
  });

  it("normalises existing whitespace between tags", () => {
    expect(formatXmlText("<a>\n\n      <b/>\n</a>")).toBe("<a>\n  <b/>\n</a>");
  });

  it("returns invalid or empty input unchanged", () => {
    expect(formatXmlText("<a><b></a>")).toBe("<a><b></a>");
    expect(formatXmlText("")).toBe("");
    expect(formatXmlText("not xml")).toBe("not xml");
  });
});
