import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MockEnvironment, MockSummary } from "../../ipc/bindings";
import { useTabsStore } from "../../shell/tabsStore";
import { useToastStore } from "../../ui/toastStore";
import * as api from "./api";
import { MockTab } from "./MockTab";
import { newRoute } from "./mockModel";
import { useMockStore } from "./mockStore";

vi.mock("./api");

const stored = (): MockEnvironment => ({
  name: "Users",
  port: 0,
  bind: "loopback",
  prefix: "",
  latency_ms: 0,
  cors: true,
  headers: [],
  routes: [{ ...newRoute(), id: "r1", endpoint: "users/:id" }],
});

const summary = (over: Partial<MockSummary> = {}): MockSummary => ({
  file: "users.mock.yaml",
  name: "Users",
  port: 0,
  routes: 1,
  running: false,
  url: null,
  error: null,
  ...over,
});

const show = () =>
  render(<MockTab collectionId="c1" file="users.mock.yaml" tabId="t1" />);

describe("MockTab", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    useToastStore.setState({ toasts: [] });
    useMockStore.setState({ byCollection: { c1: [summary()] }, logs: {} });
    useTabsStore.setState({
      tabs: [
        {
          id: "t1",
          kind: "mock",
          title: "Users",
          mock: { collectionId: "c1", file: "users.mock.yaml" },
        },
      ],
    });
    vi.mocked(api.readMock).mockResolvedValue(stored());
    vi.mocked(api.listMocks).mockImplementation(
      async () => useMockStore.getState().byCollection.c1 as MockSummary[],
    );
    vi.mocked(api.mockLog).mockResolvedValue([]);
  });

  it("loads the mock and lists its routes", async () => {
    show();
    expect(await screen.findByRole("heading", { name: "Users" })).toBeVisible();
    expect(
      within(screen.getByRole("list", { name: "Routes" })).getByText(
        "GET /users/:id",
      ),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("edits a response and saves the whole mock", async () => {
    vi.mocked(api.saveMock).mockResolvedValue(summary());
    show();
    const status = await screen.findByLabelText("Response 1 status");
    await userEvent.clear(status);
    await userEvent.type(status, "201");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveMock).toHaveBeenCalledTimes(1);
    const [, file, sent] = vi.mocked(api.saveMock).mock.calls[0]!;
    expect(file).toBe("users.mock.yaml");
    expect(sent.routes?.[0]?.responses[0]?.status).toBe(201);
  });

  it("refuses to save an invalid draft and says why", async () => {
    show();
    const status = await screen.findByLabelText("Response 1 status");
    await userEvent.clear(status);
    await userEvent.type(status, "99");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveMock).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts[0]?.message).toMatch(/status/);
  });

  it("adds a rule to a response and a route to the mock", async () => {
    show();
    await userEvent.click(
      await screen.findByRole("button", { name: "Add rule" }),
    );
    expect(screen.getByLabelText("Rule 1 target")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Add route" }));
    expect(screen.getAllByRole("button", { name: /^GET \// })).toHaveLength(2);
  });

  it("starts the mock, saving unsaved changes first, and shows its URL", async () => {
    vi.mocked(api.saveMock).mockResolvedValue(summary());
    vi.mocked(api.startMock).mockImplementation(async () => {
      useMockStore.setState({
        byCollection: {
          c1: [summary({ running: true, url: "http://127.0.0.1:4000" })],
        },
      });
      return "http://127.0.0.1:4000";
    });
    show();
    await userEvent.type(await screen.findByLabelText("Prefix"), "api");
    await userEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(api.saveMock).toHaveBeenCalled();
    expect(api.startMock).toHaveBeenCalledWith("c1", "users.mock.yaml");
    expect(await screen.findByText("http://127.0.0.1:4000/api")).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop" })).toBeVisible();
  });

  it("shows the request log and clears it", async () => {
    useMockStore.setState({
      logs: {
        "c1/users.mock.yaml": [
          {
            id: 1,
            at_ms: 0,
            method: "GET",
            path: "/users/1",
            query: "",
            headers: [{ name: "host", value: "x", enabled: true }],
            body_text: "",
            body_truncated: false,
            remote: "r",
            route_id: "r1",
            response_id: "s",
            status: 200,
            duration_ms: 3,
          },
        ],
      },
    });
    vi.mocked(api.mockLog).mockResolvedValue(
      [...(useMockStore.getState().logs["c1/users.mock.yaml"] ?? [])],
    );
    vi.mocked(api.clearMockLog).mockResolvedValue(null);
    show();
    await screen.findByRole("heading", { name: "Users" });
    await userEvent.click(screen.getByRole("tab", { name: /Log/ }));
    await userEvent.click(
      screen.getByRole("button", { name: /GET \/users\/1/ }),
    );
    expect(
      screen.getByRole("region", { name: "Request detail" }),
    ).toHaveTextContent("host");
    await userEvent.click(screen.getByRole("button", { name: "Clear log" }));
    expect(api.clearMockLog).toHaveBeenCalledWith("c1", "users.mock.yaml");
  });
});
