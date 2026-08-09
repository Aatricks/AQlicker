import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AqlickerApi, BootstrapPayload } from "../api/aqlicker";
import {
  DEFAULT_CONFIG,
  DEFAULT_PRESET,
  type AppConfig,
  type Preset,
} from "../domain/config";
import { useConfig } from "./useConfig";

function bootstrap(): BootstrapPayload {
  return {
    config: {
      ...DEFAULT_CONFIG,
      presets: [
        {
          ...DEFAULT_PRESET,
          keys: [{ key: "KeyA", weight: 1, cooldownMs: 0 }],
        },
      ],
    },
    recoveryNotice: null,
    permission: { granted: true, sameIntegrityOnly: false },
    shortcut: {
      shortcut: DEFAULT_CONFIG.globalShortcut,
      registered: true,
      error: null,
    },
    cycleShortcut: null,
    run: {
      status: "idle",
      mode: null,
      elapsedMs: 0,
      remainingMs: null,
      successfulPresses: 0,
      paused: false,
      waitingForApp: null,
      stopReason: null,
      error: null,
    },
  };
}

function fakeApi(payload = bootstrap()) {
  return {
    bootstrap: vi.fn(async () => payload),
    saveConfig: vi.fn(async () => undefined),
    setShortcut: vi.fn(async (shortcut: string) => shortcut),
    setCycleShortcut: vi.fn(async (shortcut: string | null) => shortcut),
    listenConfig: vi.fn(async (handler: (config: AppConfig) => void) => {
      configHandlers.push(handler);
      return () => undefined;
    }),
  } as unknown as AqlickerApi;
}

/** Filled by whichever `fakeApi` the test under way created. */
let configHandlers: Array<(config: AppConfig) => void> = [];

function emitConfig(config: AppConfig) {
  act(() => configHandlers.forEach((handler) => handler(config)));
}

/**
 * Asserts on the whole `presets` array, not just one field of it: a save that
 * wrote a stale document would still satisfy a per-field matcher.
 */
function onlyPresetWith(overrides: Partial<Preset>) {
  return expect.objectContaining({
    presets: [
      {
        ...DEFAULT_PRESET,
        keys: [{ key: "KeyA", weight: 1, cooldownMs: 0 }],
        ...overrides,
      },
    ],
  });
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  configHandlers = [];
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useConfig", () => {
  it("loads the bootstrap draft and debounces only valid changes for 250 ms", async () => {
    const api = fakeApi();
    const { result } = renderHook(() => useConfig(api));

    await waitFor(() => expect(result.current.config).not.toBeNull());
    expect(api.saveConfig).not.toHaveBeenCalled();

    vi.useFakeTimers();
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(249));
    expect(api.saveConfig).not.toHaveBeenCalled();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 150 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(249));
    expect(api.saveConfig).not.toHaveBeenCalled();

    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(api.saveConfig).toHaveBeenLastCalledWith(
      onlyPresetWith({ timer: { intervalMs: 150 } }),
    );

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 39 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(300));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(result.current.errors["timer.intervalMs"]).toMatch(/40/);
  });

  it("never writes one preset's contents into another's slot across an in-flight save", async () => {
    const payload = bootstrap();
    payload.config = {
      ...payload.config,
      presets: [
        { ...DEFAULT_PRESET, id: "first", name: "First" },
        { ...DEFAULT_PRESET, id: "second", name: "Second" },
      ],
      activePresetId: "first",
    };
    const firstSave = deferred<void>();
    let durableConfig = payload.config;
    const api = fakeApi(payload);
    vi.mocked(api.saveConfig)
      .mockImplementationOnce(async (candidate) => {
        await firstSave.promise;
        durableConfig = candidate;
      })
      .mockImplementation(async (candidate) => {
        durableConfig = candidate;
      });
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    // Edit the first preset, let its save leave, then switch and edit the
    // second one while that save is still outstanding.
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 111 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.updateConfig((current) => ({
        ...current,
        activePresetId: "second",
      }));
    });
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 222 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));

    await act(async () => {
      firstSave.resolve();
      await firstSave.promise;
    });

    const expected = {
      ...payload.config,
      activePresetId: "second",
      presets: [
        { ...DEFAULT_PRESET, id: "first", name: "First", timer: { intervalMs: 111 } },
        { ...DEFAULT_PRESET, id: "second", name: "Second", timer: { intervalMs: 222 } },
      ],
    };
    expect(api.saveConfig).toHaveBeenLastCalledWith(expected);
    expect(durableConfig).toEqual(expected);
  });

  it.each([
    [
      "deleting",
      (config: AppConfig): AppConfig => ({
        ...config,
        activePresetId: "second",
        presets: config.presets.filter(({ id }) => id !== "first"),
      }),
    ],
    [
      "renaming",
      (config: AppConfig): AppConfig => ({
        ...config,
        presets: config.presets.map((preset) =>
          preset.id === "second" ? { ...preset, name: "Renamed" } : preset,
        ),
      }),
    ],
  ])(
    "keeps the document whole when %s a preset lands mid-save",
    async (_label, mutate) => {
      const payload = bootstrap();
      payload.config = {
        ...payload.config,
        activePresetId: "first",
        presets: [
          { ...DEFAULT_PRESET, id: "first", name: "First" },
          { ...DEFAULT_PRESET, id: "second", name: "Second" },
        ],
      };
      const firstSave = deferred<void>();
      let durableConfig = payload.config;
      const api = fakeApi(payload);
      vi.mocked(api.saveConfig)
        .mockImplementationOnce(async (candidate) => {
          await firstSave.promise;
          durableConfig = candidate;
        })
        .mockImplementation(async (candidate) => {
          durableConfig = candidate;
        });
      const { result } = renderHook(() => useConfig(api));
      await waitFor(() => expect(result.current.config).not.toBeNull());
      vi.useFakeTimers();

      act(() => {
        result.current.updatePreset((current) => ({
          ...current,
          timer: { intervalMs: 111 },
        }));
      });
      await act(async () => vi.advanceTimersByTimeAsync(250));
      expect(api.saveConfig).toHaveBeenCalledTimes(1);

      // The structural change lands while that save is still outstanding.
      act(() => {
        result.current.updateConfig(mutate);
      });
      await act(async () => vi.advanceTimersByTimeAsync(250));
      await act(async () => {
        firstSave.resolve();
        await firstSave.promise;
      });

      const expected = mutate({
        ...payload.config,
        presets: payload.config.presets.map((preset) =>
          preset.id === "first"
            ? { ...preset, timer: { intervalMs: 111 } }
            : preset,
        ),
      });
      expect(api.saveConfig).toHaveBeenLastCalledWith(expected);
      expect(durableConfig).toEqual(expected);
    },
  );

  it("cancels a pending save when the hook unmounts", async () => {
    const api = fakeApi();
    const { result, unmount } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    vi.useFakeTimers();
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        stopAfter: 300,
      }));
    });
    unmount();
    await vi.advanceTimersByTimeAsync(300);

    expect(api.saveConfig).not.toHaveBeenCalled();
  });

  it("persists removal of the final key while retaining the start-only error", async () => {
    const api = fakeApi();
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    vi.useFakeTimers();
    act(() => {
      result.current.updatePreset((current) => ({ ...current, keys: [] }));
    });
    expect(result.current.errors.keys).toBeUndefined();
    expect(result.current.startErrors.keys).toBe("Choose at least one key");

    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledWith(
      onlyPresetWith({ keys: [] }),
    );
  });

  it("keeps an unsaved edit when the backend switches preset underneath it", async () => {
    const payload = bootstrap();
    payload.config = {
      ...payload.config,
      presets: [
        { ...DEFAULT_PRESET, id: "first", name: "First" },
        { ...DEFAULT_PRESET, id: "second", name: "Second" },
      ],
      activePresetId: "first",
    };
    const durable = payload.config;
    const api = fakeApi(payload);
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    vi.useFakeTimers();
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        name: "Renamed",
      }));
    });
    // The cycle hotkey fires before the 250 ms debounce, so Rust saves and
    // echoes the document it last knew, which predates the rename.
    emitConfig({ ...durable, activePresetId: "second" });

    expect(result.current.config?.activePresetId).toBe("second");
    expect(result.current.config?.presets[0].name).toBe("Renamed");

    // The merged draft differs from what is on disk, so it must be saved.
    await act(async () => vi.advanceTimersByTimeAsync(300));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(api.saveConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({
        activePresetId: "second",
        presets: [
          { ...DEFAULT_PRESET, id: "first", name: "Renamed" },
          { ...DEFAULT_PRESET, id: "second", name: "Second" },
        ],
      }),
    );
  });

  it("ignores a backend switch to a preset the draft has already deleted", async () => {
    const payload = bootstrap();
    payload.config = {
      ...payload.config,
      presets: [
        { ...DEFAULT_PRESET, id: "first", name: "First" },
        { ...DEFAULT_PRESET, id: "second", name: "Second" },
      ],
      activePresetId: "first",
    };
    const durable = payload.config;
    const api = fakeApi(payload);
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    act(() => {
      result.current.updateConfig((current) => ({
        ...current,
        presets: current.presets.filter((preset) => preset.id !== "second"),
      }));
    });
    emitConfig({ ...durable, activePresetId: "second" });

    // Adopting it would strand the draft on an unresolvable preset, which no
    // control on screen can fix.
    expect(result.current.config?.activePresetId).toBe("first");
    expect(result.current.errors.activePresetId).toBeUndefined();
  });

  it("adopts a backend change without writing it back when nothing is pending", async () => {
    const payload = bootstrap();
    payload.config = {
      ...payload.config,
      presets: [
        { ...DEFAULT_PRESET, id: "first", name: "First" },
        { ...DEFAULT_PRESET, id: "second", name: "Second" },
      ],
      activePresetId: "first",
    };
    const durable = payload.config;
    const api = fakeApi(payload);
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    vi.useFakeTimers();
    emitConfig({
      ...durable,
      activePresetId: "second",
      presetCycleShortcut: "CommandOrControl+Alt+9",
    });

    expect(result.current.config?.activePresetId).toBe("second");
    expect(result.current.config?.presetCycleShortcut).toBe(
      "CommandOrControl+Alt+9",
    );
    // The event carries the document Rust has already written, so the durable
    // marker moves with it and the save queue stays quiet.
    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(api.saveConfig).not.toHaveBeenCalled();
  });

  it("stores and clears the preset-cycling shortcut", async () => {
    const api = fakeApi();
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    await act(async () => {
      await result.current.registerCycleShortcut("CommandOrControl+Alt+P");
    });
    expect(api.setCycleShortcut).toHaveBeenCalledWith("CommandOrControl+Alt+P");
    expect(result.current.config?.presetCycleShortcut).toBe(
      "CommandOrControl+Alt+P",
    );

    await act(async () => {
      await result.current.registerCycleShortcut(null);
    });
    expect(api.setCycleShortcut).toHaveBeenLastCalledWith(null);
    expect(result.current.config?.presetCycleShortcut).toBeNull();
  });

  it("registers a shortcut first and retains the previous value on rejection", async () => {
    const api = fakeApi();
    let resolveRegistration!: (value: string) => void;
    const registration = new Promise<string>((resolve) => {
      resolveRegistration = resolve;
    });
    vi.mocked(api.setShortcut).mockReturnValueOnce(registration);
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    let pending!: Promise<string>;
    act(() => {
      pending = result.current.registerShortcut("CommandOrControl+Alt+P");
    });
    expect(api.setShortcut).toHaveBeenCalledWith("CommandOrControl+Alt+P");
    expect(result.current.config?.globalShortcut).toBe(
      DEFAULT_CONFIG.globalShortcut,
    );

    await act(async () => {
      resolveRegistration("CommandOrControl+Alt+P");
      await pending;
    });
    expect(result.current.config?.globalShortcut).toBe(
      "CommandOrControl+Alt+P",
    );

    vi.mocked(api.setShortcut).mockRejectedValueOnce(new Error("conflict"));
    await expect(
      result.current.registerShortcut("CommandOrControl+Shift+P"),
    ).rejects.toThrow("conflict");
    expect(result.current.config?.globalShortcut).toBe(
      "CommandOrControl+Alt+P",
    );
  });

  it("compensates when an in-flight save settles after the draft returns to the loaded config", async () => {
    const payload = bootstrap();
    const firstSave = deferred<void>();
    let durableConfig = payload.config;
    const api = fakeApi(payload);
    vi.mocked(api.saveConfig)
      .mockImplementationOnce(async (candidate) => {
        await firstSave.promise;
        durableConfig = candidate;
      })
      .mockImplementation(async (candidate) => {
        durableConfig = candidate;
      });
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 39 },
      }));
    });
    act(() => {
      result.current.updateConfig(payload.config);
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));

    await act(async () => {
      firstSave.resolve();
      await firstSave.promise;
    });

    expect(api.saveConfig).toHaveBeenCalledTimes(2);
    expect(api.saveConfig).toHaveBeenLastCalledWith(payload.config);
    expect(durableConfig).toEqual(payload.config);
  });

  it("coalesces rapid valid drafts behind one in-flight save without overlap", async () => {
    const payload = bootstrap();
    const firstSave = deferred<void>();
    let durableConfig = payload.config;
    let activeSaves = 0;
    let maximumActiveSaves = 0;
    let saveNumber = 0;
    const api = fakeApi(payload);
    vi.mocked(api.saveConfig).mockImplementation(async (candidate) => {
      saveNumber += 1;
      activeSaves += 1;
      maximumActiveSaves = Math.max(maximumActiveSaves, activeSaves);
      try {
        if (saveNumber === 1) await firstSave.promise;
        durableConfig = candidate;
      } finally {
        activeSaves -= 1;
      }
    });
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 150 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);

    await act(async () => {
      firstSave.resolve();
      await firstSave.promise;
    });

    expect(api.saveConfig).toHaveBeenCalledTimes(2);
    expect(api.saveConfig).toHaveBeenLastCalledWith(
      onlyPresetWith({ timer: { intervalMs: 150 } }),
    );
    expect(maximumActiveSaves).toBe(1);
    expect(durableConfig.presets[0].timer.intervalMs).toBe(150);
  });

  it("continues with the latest queued draft after an in-flight save fails", async () => {
    const payload = bootstrap();
    const firstSave = deferred<void>();
    let durableConfig = payload.config;
    const api = fakeApi(payload);
    vi.mocked(api.saveConfig)
      .mockImplementationOnce(async () => {
        await firstSave.promise;
      })
      .mockImplementation(async (candidate) => {
        durableConfig = candidate;
      });
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 150 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));

    await act(async () => {
      firstSave.reject(new Error("first save failed"));
      await firstSave.promise.catch(() => undefined);
    });

    expect(api.saveConfig).toHaveBeenCalledTimes(2);
    expect(api.saveConfig).toHaveBeenLastCalledWith(
      onlyPresetWith({ timer: { intervalMs: 150 } }),
    );
    expect(durableConfig.presets[0].timer.intervalMs).toBe(150);
    expect(result.current.saveError).toBeNull();
  });

  it("surfaces a latest failure and retries that draft when it is submitted again", async () => {
    const payload = bootstrap();
    let durableConfig = payload.config;
    const api = fakeApi(payload);
    vi.mocked(api.saveConfig)
      .mockRejectedValueOnce(new Error("save failed"))
      .mockImplementation(async (candidate) => {
        durableConfig = candidate;
      });
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(result.current.saveError).toBe("save failed");

    act(() => {
      result.current.updateConfig((current) => ({ ...current }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));

    expect(api.saveConfig).toHaveBeenCalledTimes(2);
    expect(durableConfig.presets[0].timer.intervalMs).toBe(125);
    expect(result.current.saveError).toBeNull();
  });

  it("clears a terminal save error when the draft returns to the durable config", async () => {
    const payload = bootstrap();
    const api = fakeApi(payload);
    vi.mocked(api.saveConfig).mockRejectedValueOnce(new Error("save failed"));
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(result.current.saveError).toBe("save failed");

    act(() => {
      result.current.updateConfig(payload.config);
    });

    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(result.current.saveError).toBeNull();
  });

  it("does not save again when the latest draft is already settled", async () => {
    const api = fakeApi();
    const { result } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.updateConfig((current) => ({ ...current }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(1000));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
  });

  it("ignores an in-flight save callback after unmount", async () => {
    const api = fakeApi();
    const saving = deferred<void>();
    vi.mocked(api.saveConfig).mockReturnValueOnce(saving.promise);
    const { result, unmount } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());
    vi.useFakeTimers();

    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    act(() => {
      result.current.updatePreset((current) => ({
        ...current,
        timer: { intervalMs: 150 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(250));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);

    const lastVisibleError = result.current.saveError;
    unmount();

    saving.reject(new Error("after unmount"));
    await saving.promise.catch(() => undefined);
    expect(result.current.saveError).toBe(lastVisibleError);
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
  });
});
