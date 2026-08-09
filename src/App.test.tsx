import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type {
  AqlickerApi,
  BootstrapPayload,
  PermissionStatus,
  RunSnapshot,
} from "./api/aqlicker";
import { DEFAULT_CONFIG, DEFAULT_PRESET, type AppConfig } from "./domain/config";
import { IDLE_SNAPSHOT } from "./hooks/useRunState";
import App from "./App";

function fakeApi(overrides: Partial<BootstrapPayload> = {}) {
  const payload: BootstrapPayload = {
    config: DEFAULT_CONFIG,
    recoveryNotice: null,
    permission: { granted: true, sameIntegrityOnly: false },
    shortcut: {
      shortcut: DEFAULT_CONFIG.globalShortcut,
      registered: true,
      error: null,
    },
    run: IDLE_SNAPSHOT,
    ...overrides,
  };

  const handlers: Array<(state: RunSnapshot) => void> = [];
  const unlisten = vi.fn();
  const api = {
    bootstrap: vi.fn(async () => payload),
    saveConfig: vi.fn(async () => undefined),
    startRun: vi.fn(async () => IDLE_SNAPSHOT),
    stopRun: vi.fn(async () => IDLE_SNAPSHOT),
    requestAccess: vi.fn(
      async (): Promise<PermissionStatus> => ({
        granted: true,
        sameIntegrityOnly: false,
      }),
    ),
    permissionStatus: vi.fn(
      async (): Promise<PermissionStatus> => payload.permission,
    ),
    setShortcut: vi.fn(async (shortcut: string) => shortcut),
    listApps: vi.fn(async () => [{ id: "com.apple.TextEdit", name: "TextEdit" }]),
    listenRunState: vi.fn(async (handler: (state: RunSnapshot) => void) => {
      handlers.push(handler);
      return unlisten;
    }),
  } satisfies AqlickerApi;

  return {
    api,
    unlisten,
    emit: (state: RunSnapshot) =>
      act(() => handlers.forEach((handler) => handler(state))),
  };
}

function naturalConfig(): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    presets: [
      {
        ...DEFAULT_PRESET,
        keys: [{ key: "Space", weight: 3, cooldownMs: 0 }],
        mode: "natural",
        stopAfter: 3_600,
      },
    ],
  };
}

function runningState(overrides: Partial<RunSnapshot> = {}): RunSnapshot {
  return {
    ...IDLE_SNAPSHOT,
    status: "running",
    mode: "natural",
    ...overrides,
  };
}

describe("App", () => {
  it("shows the paused target application in the header and keeps Stop available", async () => {
    const { api, emit } = fakeApi();
    render(<App api={api} />);
    await screen.findByRole("heading", { name: "AQlicker" });

    emit(
      runningState({
        elapsedMs: 4_000,
        successfulPresses: 3,
        paused: true,
        waitingForApp: "TextEdit",
      }),
    );

    expect(screen.getByRole("status")).toHaveTextContent(
      "Paused · waiting for TextEdit",
    );
    expect(screen.getByText("Paused — waiting for TextEdit")).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("renders the idle AQlicker shell and required-key validation", async () => {
    render(<App api={fakeApi().api} />);

    expect(screen.getByRole("heading", { name: "AQlicker" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    // Reported both inline on the field and as a Start prerequisite.
    expect(await screen.findAllByText("Choose at least one key")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Add key" })).toBeEnabled();
  });

  it("composes mode weights and the shared automatic-stop draft", async () => {
    render(
      <App
        api={
          fakeApi({
            config: {
              ...DEFAULT_CONFIG,
              presets: [
                {
                  ...DEFAULT_PRESET,
                  keys: [{ key: "Space", weight: 3, cooldownMs: 0 }],
                },
              ],
            },
          }).api
        }
      />,
    );
    await screen.findByRole("button", { name: "Add key" });

    fireEvent.click(screen.getByRole("button", { name: "Natural" }));
    expect(
      screen.getByRole("spinbutton", { name: "Space frequency weight" }),
    ).toHaveValue(3);

    fireEvent.click(screen.getByRole("button", { name: "1 hour" }));
    expect(screen.getByRole("spinbutton", { name: "Hours" })).toHaveValue(1);
    expect(
      screen.getByRole("heading", { name: "Global shortcut" }),
    ).toBeVisible();
  });

  it("edits and switches presets, and locks the preset control during a run", async () => {
    const config: AppConfig = {
      ...DEFAULT_CONFIG,
      activePresetId: "first",
      presets: [
        {
          ...DEFAULT_PRESET,
          id: "first",
          name: "First",
          keys: [{ key: "KeyA", weight: 1, cooldownMs: 0 }],
          timer: { intervalMs: 111 },
        },
        {
          ...DEFAULT_PRESET,
          id: "second",
          name: "Second",
          keys: [{ key: "Space", weight: 1, cooldownMs: 0 }],
          timer: { intervalMs: 222 },
        },
      ],
    };
    const { api, emit } = fakeApi({ config });
    render(<App api={api} />);

    const select = await screen.findByRole("combobox", {
      name: /Active preset/,
    });
    expect(
      screen.getByRole("spinbutton", { name: "Timer interval (ms)" }),
    ).toHaveValue(111);

    fireEvent.change(select, { target: { value: "second" } });
    expect(
      screen.getByRole("spinbutton", { name: "Timer interval (ms)" }),
    ).toHaveValue(222);

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(api.startRun).toHaveBeenCalledWith({
        ...config,
        activePresetId: "second",
      }),
    );
    emit(runningState({ mode: "timer" }));

    expect(select).toBeDisabled();
    for (const name of [
      "New preset",
      "Duplicate preset",
      "Rename preset",
      "Delete preset",
    ]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
  });

  it("announces Unavailable rather than Ready when bootstrap fails", async () => {
    const { api } = fakeApi();
    vi.mocked(api.bootstrap).mockRejectedValueOnce(new Error("offline"));
    render(<App api={api} />);

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Unavailable"),
    );
    expect(
      screen.getByText("Could not load settings: offline"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Ready")).not.toBeInTheDocument();
  });

  it("locks settings and counts down a natural run", async () => {
    const { api, emit } = fakeApi({ config: naturalConfig() });
    render(<App api={api} />);

    fireEvent.click(await screen.findByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(api.startRun).toHaveBeenCalledWith(naturalConfig()),
    );

    emit(
      runningState({
        elapsedMs: 5_000,
        remainingMs: 3_595_000,
        successfulPresses: 21,
      }),
    );

    expect(screen.getByText("Running · Natural")).toBeVisible();
    expect(screen.getByText("Natural mode running")).toBeVisible();
    expect(screen.getByText("59:55 remaining")).toBeVisible();
    expect(screen.getByText("21 presses")).toBeVisible();
    expect(screen.getByRole("button", { name: "Add key" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Record shortcut" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("adopts a run already in progress from the bootstrap snapshot", async () => {
    const { api } = fakeApi({
      config: naturalConfig(),
      run: runningState({ elapsedMs: 12_000, successfulPresses: 48 }),
    });
    render(<App api={api} />);

    expect(await screen.findByText("Running · Natural")).toBeVisible();
    expect(screen.getByText("48 presses")).toBeVisible();
    expect(screen.getByRole("button", { name: "Add key" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(api.startRun).not.toHaveBeenCalled();
  });

  it("locks and unlocks from backend events without ever resuming a run", async () => {
    const { api, emit } = fakeApi({ config: naturalConfig() });
    // The reply is still in flight while presses keep being published.
    vi.mocked(api.stopRun).mockImplementation(() => new Promise(() => undefined));
    render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    emit(runningState({ elapsedMs: 1_000, successfulPresses: 2 }));
    expect(screen.getByRole("button", { name: "Add key" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(api.stopRun).toHaveBeenCalledTimes(1));

    // A press tick published between the click and the terminal event must not
    // re-arm Stop.
    emit(runningState({ elapsedMs: 2_000, successfulPresses: 3 }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.stopRun).toHaveBeenCalledTimes(1);

    emit({ ...IDLE_SNAPSHOT, stopReason: "requested" });
    expect(screen.getByRole("button", { name: "Add key" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
    expect(api.startRun).not.toHaveBeenCalled();
  });

  it("explains a missing permission, requests it on demand, and rechecks on focus", async () => {
    const { api } = fakeApi({
      config: naturalConfig(),
      permission: { granted: false, sameIntegrityOnly: false },
    });
    render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    expect(screen.getByText("Grant input permission")).toBeVisible();
    expect(api.requestAccess).not.toHaveBeenCalled();

    vi.mocked(api.requestAccess).mockResolvedValueOnce({
      granted: false,
      sameIntegrityOnly: false,
    });
    fireEvent.click(screen.getByRole("button", { name: "Request access" }));
    await waitFor(() => expect(api.requestAccess).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();

    vi.mocked(api.permissionStatus).mockResolvedValue({
      granted: true,
      sameIntegrityOnly: false,
    });
    fireEvent.focus(window);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
  });

  it("stops rechecking permission after unmount", async () => {
    const { api, unlisten } = fakeApi({ config: naturalConfig() });
    const view = render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    view.unmount();
    fireEvent.focus(window);
    expect(api.permissionStatus).not.toHaveBeenCalled();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("clears pending timers when the window unmounts", async () => {
    vi.useFakeTimers();
    try {
      const { api } = fakeApi({ config: naturalConfig() });
      const view = render(<App api={api} />);
      await act(async () => {});

      fireEvent.click(screen.getByRole("button", { name: "15 minutes" }));
      expect(vi.getTimerCount()).toBeGreaterThan(0);

      view.unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the previous shortcut visible when registration conflicts", async () => {
    const { api } = fakeApi({ config: naturalConfig() });
    vi.mocked(api.setShortcut).mockRejectedValueOnce({
      code: "shortcut-conflict",
    });
    render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    fireEvent.click(screen.getByRole("button", { name: "Record shortcut" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press shortcut" }), {
      code: "KeyJ",
      key: "j",
      metaKey: true,
      shiftKey: true,
    });

    expect(
      await screen.findByText(
        "That shortcut could not be registered. Try another one.",
      ),
    ).toBeVisible();
    expect(screen.getByText("CommandOrControl+Shift+K")).toBeVisible();
  });

  it("blocks Start until an unregistered shortcut is recorded successfully", async () => {
    const { api } = fakeApi({
      config: naturalConfig(),
      shortcut: {
        shortcut: DEFAULT_CONFIG.globalShortcut,
        registered: false,
        error: "shortcut-conflict",
      },
    });
    render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    expect(screen.getByText("Register the global shortcut")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Record shortcut" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press shortcut" }), {
      code: "KeyJ",
      key: "j",
      metaKey: true,
      shiftKey: true,
    });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
  });

  it("names the failed key after an input failure and lets the notice be dismissed", async () => {
    const { api, emit } = fakeApi({ config: naturalConfig() });
    render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    emit({
      ...IDLE_SNAPSHOT,
      status: "failed",
      stopReason: "inputFailure",
      error: {
        code: "input-failure",
        key: "Space",
        message: "CGEvent post failed",
      },
    });

    expect(
      screen.getByText(/AQlicker could not send the Space key/),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText(/AQlicker could not send/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
  });

  it("reports corrupt-configuration recovery without blocking configuration", async () => {
    const { api } = fakeApi({
      config: naturalConfig(),
      recoveryNotice: { code: "corrupt-config-recovered" },
    });
    render(<App api={api} />);

    expect(
      await screen.findByText(
        "Saved settings could not be read, so AQlicker kept the original file and loaded defaults.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Add key" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(
      screen.queryByText(/Saved settings could not be read/),
    ).not.toBeInTheDocument();
  });

  it("reports a worker panic and falls back to the backend message for unknown codes", async () => {
    const { api, emit } = fakeApi({ config: naturalConfig() });
    render(<App api={api} />);
    await screen.findByRole("button", { name: "Add key" });

    emit({
      ...IDLE_SNAPSHOT,
      status: "failed",
      stopReason: "workerPanic",
      error: { code: "worker-panic", key: null, message: "scheduler panicked" },
    });
    expect(
      screen.getByText(
        "The run stopped unexpectedly and no key is left held down.",
      ),
    ).toBeVisible();

    emit({
      ...IDLE_SNAPSHOT,
      status: "failed",
      error: { code: "wait-timeout", key: null, message: "shutdown timed out" },
    });
    expect(
      screen.getByText("AQlicker timed out waiting for the run to finish."),
    ).toBeVisible();
  });

  it("never shows a raw backend identifier for a coded rejection", async () => {
    const { api } = fakeApi({ config: naturalConfig() });
    vi.mocked(api.startRun).mockRejectedValueOnce({
      code: "run-terminal-pending",
    });
    render(<App api={api} />);

    fireEvent.click(await screen.findByRole("button", { name: "Start" }));

    expect(
      await screen.findByText(
        "The previous run is still finishing. Try again in a moment.",
      ),
    ).toBeVisible();
    expect(screen.queryByText("run-terminal-pending")).not.toBeInTheDocument();
  });

  it("falls back to generic wording instead of echoing an unmapped code", async () => {
    const { api } = fakeApi({ config: naturalConfig() });
    vi.mocked(api.startRun).mockRejectedValueOnce({ code: "brand-new-code" });
    render(<App api={api} />);

    fireEvent.click(await screen.findByRole("button", { name: "Start" }));

    expect(
      await screen.findByText("AQlicker could not complete that action."),
    ).toBeVisible();
    expect(screen.queryByText("brand-new-code")).not.toBeInTheDocument();
  });

  it("explains a rejected Start using the backend error code", async () => {
    const { api } = fakeApi({ config: naturalConfig() });
    vi.mocked(api.startRun).mockRejectedValueOnce({ code: "escape-unavailable" });
    render(<App api={api} />);

    fireEvent.click(await screen.findByRole("button", { name: "Start" }));

    expect(
      await screen.findByText(
        "Escape could not be reserved as the emergency stop, so the run did not begin.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Add key" })).toBeEnabled();
  });
});
