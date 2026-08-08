import { useCallback, useEffect, useRef, useState } from "react";
import {
  aqlickerApi,
  type AqlickerApi,
  type RunError,
  type RunSnapshot,
} from "../api/aqlicker";
import type { AppConfig } from "../domain/config";

export const IDLE_SNAPSHOT: RunSnapshot = {
  status: "idle",
  mode: null,
  elapsedMs: 0,
  remainingMs: null,
  successfulPresses: 0,
  stopReason: null,
  error: null,
};

function toRunError(error: unknown): RunError {
  if (typeof error === "object" && error !== null && "code" in error) {
    return { code: String((error as { code: unknown }).code), key: null, message: "" };
  }
  return {
    code: "start-failed",
    key: null,
    message: error instanceof Error ? error.message : "",
  };
}

/**
 * Mirrors the Rust run controller. Rust owns elapsed and remaining time, so the
 * hook never derives them locally. Snapshots reach it from three places, in
 * descending authority: run-state events, the snapshot an invoke resolves with,
 * and the bootstrap payload. Tauri does not replay emits and the bootstrap
 * request races listener registration, so the latter two are needed to notice a
 * run that was already active — but they are only applied when no event has
 * arrived since they were requested, so a slow reply cannot undo a newer event.
 */
export function useRunState(
  api: AqlickerApi = aqlickerApi,
  initial?: RunSnapshot | null,
) {
  const [snapshot, setSnapshot] = useState<RunSnapshot>(IDLE_SNAPSHOT);
  const [error, setError] = useState<RunError | null>(null);
  const [stopPending, setStopPending] = useState(false);
  const mounted = useRef(true);
  const stopInFlight = useRef(false);
  const events = useRef(0);
  const seeded = useRef(false);

  const apply = useCallback((next: RunSnapshot) => {
    setSnapshot(next);
    setError(next.error);
    // Rust also publishes a snapshot after every successful press, so the Stop
    // latch may only be released once the run has left `running`.
    if (next.status !== "running") {
      stopInFlight.current = false;
      setStopPending(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void api
      .listenRunState((next) => {
        if (disposed) return;
        events.current += 1;
        apply(next);
      })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      mounted.current = false;
      unlisten?.();
    };
  }, [api, apply]);

  useEffect(() => {
    if (!initial || seeded.current) return;
    seeded.current = true;
    if (events.current === 0) apply(initial);
  }, [apply, initial]);

  const settle = useCallback(
    (seen: number, next: RunSnapshot) => {
      if (mounted.current && events.current === seen) apply(next);
    },
    [apply],
  );

  const start = useCallback(
    async (config: AppConfig) => {
      setError(null);
      const seen = events.current;
      try {
        settle(seen, await api.startRun(config));
      } catch (rejection) {
        if (mounted.current) setError(toRunError(rejection));
      }
    },
    [api, settle],
  );

  // Stays latched after the first Stop until Rust reports a snapshot that has
  // left `running`, so a second click cannot reach the backend. Applying the
  // reply matters when the backend was already idle: `RunController::stop`
  // publishes nothing in that case, so no event would ever release the latch.
  const stop = useCallback(async () => {
    if (stopInFlight.current) return;
    stopInFlight.current = true;
    setStopPending(true);
    const seen = events.current;
    try {
      settle(seen, await api.stopRun());
    } catch (rejection) {
      stopInFlight.current = false;
      if (mounted.current) {
        setError(toRunError(rejection));
        setStopPending(false);
      }
    }
  }, [api, settle]);

  const dismissError = useCallback(() => setError(null), []);

  return { snapshot, error, stopPending, start, stop, dismissError };
}
