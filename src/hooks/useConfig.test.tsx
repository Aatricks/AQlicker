import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AqlickerApi, BootstrapPayload } from "../api/aqlicker";
import { DEFAULT_CONFIG } from "../domain/config";
import { useConfig } from "./useConfig";

function bootstrap(): BootstrapPayload {
  return {
    config: {
      ...DEFAULT_CONFIG,
      keys: [{ key: "KeyA", weight: 1 }],
    },
    recoveryNotice: null,
    permission: { granted: true, sameIntegrityOnly: false },
    shortcut: {
      shortcut: DEFAULT_CONFIG.globalShortcut,
      registered: true,
      error: null,
    },
    run: {
      status: "idle",
      mode: null,
      elapsedMs: 0,
      remainingMs: null,
      successfulPresses: 0,
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
  } as unknown as AqlickerApi;
}

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
      result.current.updateConfig((current) => ({
        ...current,
        timer: { intervalMs: 125 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(249));
    expect(api.saveConfig).not.toHaveBeenCalled();

    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(api.saveConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ timer: { intervalMs: 125 } }),
    );

    act(() => {
      result.current.updateConfig((current) => ({
        ...current,
        timer: { intervalMs: 39 },
      }));
    });
    await act(async () => vi.advanceTimersByTimeAsync(300));
    expect(api.saveConfig).toHaveBeenCalledTimes(1);
    expect(result.current.errors["timer.intervalMs"]).toMatch(/40/);
  });

  it("cancels a pending save when the hook unmounts", async () => {
    const api = fakeApi();
    const { result, unmount } = renderHook(() => useConfig(api));
    await waitFor(() => expect(result.current.config).not.toBeNull());

    vi.useFakeTimers();
    act(() => {
      result.current.updateConfig((current) => ({
        ...current,
        stopAfter: 300,
      }));
    });
    unmount();
    await vi.advanceTimersByTimeAsync(300);

    expect(api.saveConfig).not.toHaveBeenCalled();
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
});
