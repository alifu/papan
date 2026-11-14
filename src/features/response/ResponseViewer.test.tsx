import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ResponseMeta, ScriptReport } from "../../ipc/bindings";
import { useDialogsStore } from "../../shell/dialogsStore";
import * as api from "./api";
import { ResponseViewer } from "./ResponseViewer";

vi.mock("./api", () => ({ readResponseBody: vi.fn(), saveResponseBody: vi.fn() }));

const meta: ResponseMeta = {
  request_id: "r1",
  status: 200,
  status_text: "OK",
  http_version: "HTTP/1.1",
  url: "http://x.test/a",
  headers: [{ name: "X-A", value: "1" }],
  cookies: [{ name: "sid", value: "abc", domain: null, path: "/", expires: null, http_only: true, secure: false }],
  content_type: "application/json",
  size_bytes: 1536,
  truncated: false,
  ttfb_ms: 5,
  total_ms: 12,
  timing: { dns_ms: 0, connect_ms: 0, wait_ms: 0, download_ms: 0, reused: false },
  tls_verified: true,
  unresolved: [],
};

beforeEach(() => {
  vi.mocked(api.readResponseBody).mockResolvedValue({ encoding: "text", data: '{"a":1}', truncated: false });
  vi.mocked(api.saveResponseBody).mockResolvedValue("out.json");
});

describe("ResponseViewer", () => {
  it("shows an empty state before the first send", () => {
    render(<ResponseViewer response={null} error={null} sending={false} />);
    expect(screen.getByText(/send a request/i)).toBeInTheDocument();
  });

  it("shows sending state", () => {
    render(<ResponseViewer response={null} error={null} sending />);
    expect(screen.getByText(/sending/i)).toBeInTheDocument();
  });

  it("shows the engine message for a network error", () => {
    render(<ResponseViewer response={null} error={{ code: "network", message: "Could not connect to x." }} sending={false} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Could not connect to x.");
  });

  it("shows Cancelled for a cancelled send", () => {
    render(<ResponseViewer response={null} error={{ code: "cancelled", message: "x" }} sending={false} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Cancelled");
  });

  it("shows status, time, size and the pretty body", async () => {
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    expect(screen.getByText("200 OK")).toBeInTheDocument();
    expect(screen.getByText("12 ms")).toBeInTheDocument();
    expect(screen.getByText("1.5 KB")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Tree" })).toBeInTheDocument();
    expect(api.readResponseBody).toHaveBeenCalledWith("r1");
  });

  it("lists headers and cookies", async () => {
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    await userEvent.click(screen.getByRole("tab", { name: /headers/i }));
    expect(screen.getByText("X-A")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("tab", { name: /cookies/i }));
    expect(screen.getByText("sid")).toBeInTheDocument();
  });

  it("saves to file through the command", async () => {
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Save to file..." }));
    await waitFor(() => expect(api.saveResponseBody).toHaveBeenCalledWith("r1"));
  });

  it("shows a truncation notice and TLS badge", async () => {
    render(
      <ResponseViewer
        response={{ ...meta, url: "https://x.test/a", truncated: true, tls_verified: false }}
        error={null}
        sending={false}
      />,
    );
    expect(screen.getByText("TLS not verified")).toBeInTheDocument();
    expect(await screen.findByText(/cut at the size limit/i)).toBeInTheDocument();
  });

  it("does not show the TLS badge for plain http", () => {
    render(<ResponseViewer response={{ ...meta, tls_verified: false }} error={null} sending={false} />);
    expect(screen.queryByText("TLS not verified")).not.toBeInTheDocument();
  });

  it("highlights search matches in raw mode", async () => {
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Raw" }));
    await userEvent.type(screen.getByLabelText("Find in response"), "a");
    await waitFor(() => expect(document.querySelectorAll(".search-mark-current")).toHaveLength(1));
  });

  it("lists variables that were sent as typed", () => {
    render(<ResponseViewer response={{ ...meta, unresolved: ["base_url", "token"] }} error={null} sending={false} />);
    expect(screen.getByText(/unresolved variables: base_url, token/i)).toBeInTheDocument();
  });

  it("shows no banner when everything resolved", () => {
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    expect(screen.queryByText(/unresolved variables/i)).not.toBeInTheDocument();
  });

  it("tells the user when the body was evicted", async () => {
    vi.mocked(api.readResponseBody).mockRejectedValue({ code: "not_found", message: "gone" });
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    expect(await screen.findByText(/no longer available/i)).toBeInTheDocument();
  });

  it("opens the timing popover from the time", async () => {
    render(<ResponseViewer response={{ ...meta, timing: { dns_ms: 1, connect_ms: 2, wait_ms: 3, download_ms: 4, reused: true } }} error={null} sending={false} />);
    await userEvent.click(screen.getByRole("button", { name: "12 ms" }));
    expect(screen.getByRole("dialog", { name: "Timing" })).toHaveTextContent("Connection reused");
    await userEvent.click(screen.getByRole("button", { name: "12 ms" }));
    expect(screen.queryByRole("dialog", { name: "Timing" })).toBeNull();
  });

  it("opens the cookie manager from the Cookies tab", async () => {
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    await userEvent.click(screen.getByRole("tab", { name: /cookies/i }));
    await userEvent.click(screen.getByRole("button", { name: "Manage cookies…" }));
    expect(useDialogsStore.getState().open).toBe("cookies");
  });

  describe("script reports", () => {
    const report = (over: Partial<ScriptReport>): ScriptReport => ({
      phase: "post_response",
      console: [],
      console_truncated: false,
      tests: [],
      changes: [],
      error: null,
      ...over,
    });

    it("hides script tabs when no script ran", () => {
      render(<ResponseViewer response={meta} error={null} sending={false} scripts={[]} />);
      expect(screen.queryByRole("tab", { name: /Console/ })).not.toBeInTheDocument();
    });

    it("shows a failed send together with the report of the script that ran before it", async () => {
      const scripts = [report({ phase: "pre_request", console: [{ level: "log", text: "before the failure" }] })];
      render(<ResponseViewer response={null} error={{ code: "network", message: "Could not connect." }} sending={false} scripts={scripts} />);
      expect(screen.getByRole("alert")).toHaveTextContent("Could not connect.");
      await userEvent.click(screen.getByRole("tab", { name: /Console/ }));
      expect(screen.getByText("before the failure")).toBeInTheDocument();
    });

    it("lists tests with a pass/total label", async () => {
      const scripts = [report({ tests: [{ name: "ok", passed: true, error: null }, { name: "bad", passed: false, error: "expected 1" }] })];
      render(<ResponseViewer response={meta} error={null} sending={false} scripts={scripts} />);
      await userEvent.click(screen.getByRole("tab", { name: "Tests (1/2)" }));
      expect(screen.getByText("ok")).toBeInTheDocument();
      expect(screen.getByText("expected 1")).toBeInTheDocument();
    });

    it("shows console lines with phase, truncation and secret-safe changes", async () => {
      const scripts = [
        report({
          phase: "pre_request",
          console: [{ level: "warn", text: "careful" }],
          console_truncated: true,
          changes: [
            { scope: "environment", name: "token", value: "s3cret", secret: true, removed: false },
            { scope: "globals", name: "n", value: "1", secret: false, removed: false },
          ],
        }),
      ];
      render(<ResponseViewer response={meta} error={null} sending={false} scripts={scripts} />);
      await userEvent.click(screen.getByRole("tab", { name: "Console" }));
      expect(screen.getByText(/careful/)).toBeInTheDocument();
      expect(screen.getByText("[pre-request]", { selector: ".console-line:first-child .console-phase" })).toBeInTheDocument();
      expect(screen.getByText("Output truncated.")).toBeInTheDocument();
      expect(screen.getByText(/variable token$/)).toBeInTheDocument();
      expect(screen.queryByText(/s3cret/)).not.toBeInTheDocument();
      expect(screen.getByText(/variable n = 1/)).toBeInTheDocument();
    });

    it("shows the reports and an error banner when the send was stopped", async () => {
      const scripts = [
        report({ phase: "pre_request", error: { kind: "exception", message: "boom", script: "request", line: 3 } }),
      ];
      render(<ResponseViewer response={null} error={null} sending={false} scripts={scripts} />);
      expect(screen.getByRole("alert")).toHaveTextContent("boom");
      expect(screen.getByRole("alert")).toHaveTextContent("request, line 3");
      expect(screen.getByRole("tab", { name: "Console" })).toBeInTheDocument();
      expect(screen.queryByText(/Send a request/)).not.toBeInTheDocument();
    });
  });
});
