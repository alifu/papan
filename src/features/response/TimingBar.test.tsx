import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Timing } from "../../ipc/bindings";
import { TimingBar } from "./TimingBar";

const timing: Timing = { dns_ms: 5, connect_ms: 20, wait_ms: 60, download_ms: 15, reused: false };

describe("TimingBar", () => {
  it("lists the four phases in ms", () => {
    render(<TimingBar timing={timing} />);
    for (const [label, ms] of [["DNS", 5], ["Connect + TLS", 20], ["Waiting", 60], ["Download", 15]] as const) {
      expect(screen.getByText(label).parentElement).toHaveTextContent(`${ms} ms`);
    }
    expect(screen.getByRole("img")).toHaveAttribute("aria-label", "Total 100 ms");
    expect(screen.queryByText("Connection reused")).toBeNull();
  });

  it("sizes segments by duration and skips zero phases", () => {
    const { container } = render(<TimingBar timing={{ ...timing, dns_ms: 0, connect_ms: 0, reused: true }} />);
    const segments = container.querySelectorAll<HTMLElement>(".timing-segment");
    expect([...segments].map((s) => s.dataset.phase)).toEqual(["wait_ms", "download_ms"]);
    expect(segments[0].style.flexGrow).toBe("60");
    expect(screen.getByText("Connection reused")).toBeInTheDocument();
    expect(screen.getByText(/Diagnostic only/)).toBeInTheDocument();
  });
});
