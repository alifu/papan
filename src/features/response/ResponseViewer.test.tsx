import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ResponseMeta } from "../../ipc/bindings";
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
  tls_verified: true,
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
    expect(document.querySelectorAll("mark")).toHaveLength(1);
  });

  it("tells the user when the body was evicted", async () => {
    vi.mocked(api.readResponseBody).mockRejectedValue({ code: "not_found", message: "gone" });
    render(<ResponseViewer response={meta} error={null} sending={false} />);
    expect(await screen.findByText(/no longer available/i)).toBeInTheDocument();
  });
});
