import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_CONFIG } from "../domain/config";
import { aqlickerApi, RUN_STATE_EVENT, type RunSnapshot } from "./aqlicker";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

describe("AQlicker desktop API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invoke.mockResolvedValue(undefined);
    mocks.listen.mockResolvedValue(mocks.unlisten);
  });

  it("maps every command to its exact Rust name and camelCase payload", async () => {
    await aqlickerApi.bootstrap();
    expect(mocks.invoke).toHaveBeenLastCalledWith("bootstrap");

    await aqlickerApi.saveConfig(DEFAULT_CONFIG);
    expect(mocks.invoke).toHaveBeenLastCalledWith("save_config", {
      config: DEFAULT_CONFIG,
    });

    await aqlickerApi.startRun(DEFAULT_CONFIG);
    expect(mocks.invoke).toHaveBeenLastCalledWith("start_run", {
      config: DEFAULT_CONFIG,
    });

    await aqlickerApi.stopRun();
    expect(mocks.invoke).toHaveBeenLastCalledWith("stop_run");

    await aqlickerApi.requestAccess();
    expect(mocks.invoke).toHaveBeenLastCalledWith("request_access");

    await aqlickerApi.permissionStatus();
    expect(mocks.invoke).toHaveBeenLastCalledWith("permission_status");

    await aqlickerApi.setShortcut("CommandOrControl+Alt+P");
    expect(mocks.invoke).toHaveBeenLastCalledWith("set_shortcut", {
      shortcut: "CommandOrControl+Alt+P",
    });
  });

  it("maps typed run events and returns the Tauri unsubscribe function", async () => {
    const handler = vi.fn();
    const snapshot: RunSnapshot = {
      status: "running",
      mode: "timer",
      elapsedMs: 10,
      remainingMs: null,
      successfulPresses: 1,
      stopReason: null,
      error: null,
    };

    const unsubscribe = await aqlickerApi.listenRunState(handler);
    const listener = mocks.listen.mock.calls[0][1];
    listener({ payload: snapshot });

    expect(mocks.listen).toHaveBeenCalledWith(RUN_STATE_EVENT, expect.any(Function));
    expect(handler).toHaveBeenCalledWith(snapshot);
    expect(unsubscribe).toBe(mocks.unlisten);
  });
});
