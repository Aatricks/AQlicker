import { act, render, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AqlickerApi, RunSnapshot } from "../api/aqlicker";
import { DEFAULT_CONFIG } from "../domain/config";
import { IDLE_SNAPSHOT, useRunState } from "./useRunState";

function runningSnapshot(overrides: Partial<RunSnapshot> = {}): RunSnapshot {
  return {
    ...IDLE_SNAPSHOT,
    status: "running",
    mode: "timer",
    elapsedMs: 1_000,
    successfulPresses: 4,
    ...overrides,
  };
}

function fakeApi() {
  const handlers: Array<(state: RunSnapshot) => void> = [];
  const unlisten = vi.fn();
  let resolveListen: ((dispose: () => void) => void) | null = null;

  const api = {
    startRun: vi.fn(async () => IDLE_SNAPSHOT),
    stopRun: vi.fn(async () => IDLE_SNAPSHOT),
    listenRunState: vi.fn(
      (handler: (state: RunSnapshot) => void) =>
        new Promise<() => void>((resolve) => {
          handlers.push(handler);
          resolveListen = resolve;
        }),
    ),
  } as unknown as AqlickerApi;

  return {
    api,
    unlisten,
    settleListen: () => act(() => resolveListen?.(unlisten)),
    emit: (state: RunSnapshot) =>
      act(() => handlers.forEach((handler) => handler(state))),
  };
}

describe("useRunState", () => {
  it("replaces the snapshot from backend events only", async () => {
    const { api, settleListen, emit } = fakeApi();
    const { result } = renderHook(() => useRunState(api));
    settleListen();

    expect(result.current.snapshot).toEqual(IDLE_SNAPSHOT);

    await act(async () => {
      await result.current.start(DEFAULT_CONFIG);
    });
    expect(api.startRun).toHaveBeenCalledWith(DEFAULT_CONFIG);
    expect(result.current.snapshot.status).toBe("idle");

    emit(runningSnapshot({ successfulPresses: 21 }));
    expect(result.current.snapshot.successfulPresses).toBe(21);
  });

  it("latches Stop until the next backend snapshot arrives", async () => {
    const { api, settleListen, emit } = fakeApi();
    const { result } = renderHook(() => useRunState(api));
    settleListen();
    emit(runningSnapshot());

    await act(async () => {
      await result.current.stop();
      await result.current.stop();
    });
    expect(api.stopRun).toHaveBeenCalledTimes(1);
    expect(result.current.stopPending).toBe(true);

    // A press tick keeps publishing while the run winds down.
    emit(runningSnapshot({ elapsedMs: 2_000, successfulPresses: 9 }));
    expect(result.current.stopPending).toBe(true);

    emit(IDLE_SNAPSHOT);
    expect(result.current.stopPending).toBe(false);
  });

  it("releases the Stop latch when the backend rejects the request", async () => {
    const { api, settleListen, emit } = fakeApi();
    vi.mocked(api.stopRun).mockRejectedValueOnce({ code: "service-unavailable" });
    const { result } = renderHook(() => useRunState(api));
    settleListen();
    emit(runningSnapshot());

    await act(async () => {
      await result.current.stop();
    });
    expect(result.current.stopPending).toBe(false);
    expect(result.current.error?.code).toBe("service-unavailable");
  });

  it("surfaces a rejected start as a dismissible error code", async () => {
    const { api, settleListen, emit } = fakeApi();
    vi.mocked(api.startRun).mockRejectedValueOnce({ code: "permission-required" });
    const { result } = renderHook(() => useRunState(api));
    settleListen();

    await act(async () => {
      await result.current.start(DEFAULT_CONFIG);
    });
    expect(result.current.error?.code).toBe("permission-required");

    act(() => result.current.dismissError());
    expect(result.current.error).toBeNull();

    emit(runningSnapshot());
    expect(result.current.error).toBeNull();
  });

  it("unsubscribes on unmount even when the subscription resolves late", async () => {
    const { api, unlisten, settleListen } = fakeApi();
    const { unmount } = renderHook(() => useRunState(api));

    unmount();
    settleListen();

    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });

  it("unsubscribes exactly once when the subscription resolved first", async () => {
    const { api, unlisten, settleListen } = fakeApi();
    const { unmount } = renderHook(() => useRunState(api));
    settleListen();
    await waitFor(() => expect(unlisten).not.toHaveBeenCalled());

    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("leaves no timers pending after unmount", async () => {
    vi.useFakeTimers();
    try {
      const { api, settleListen } = fakeApi();
      const { unmount } = render(<Probe api={api} />);
      settleListen();
      unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

function Probe({ api }: { api: AqlickerApi }) {
  const { snapshot } = useRunState(api);
  return <span>{snapshot.status}</span>;
}
