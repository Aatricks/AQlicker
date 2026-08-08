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
 * snapshot is only ever replaced by a backend event — invoke results are used
 * for rejection reporting alone and never overwrite a newer event.
 */
export function useRunState(api: AqlickerApi = aqlickerApi) {
  const [snapshot, setSnapshot] = useState<RunSnapshot>(IDLE_SNAPSHOT);
  const [error, setError] = useState<RunError | null>(null);
  const [stopPending, setStopPending] = useState(false);
  const mounted = useRef(true);
  const stopInFlight = useRef(false);

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void api
      .listenRunState((next) => {
        if (disposed) return;
        setSnapshot(next);
        setError(next.error);
        // Rust also publishes a snapshot after every successful press, so the
        // Stop latch may only be released once the run has left `running`.
        if (next.status !== "running") {
          stopInFlight.current = false;
          setStopPending(false);
        }
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
  }, [api]);

  const start = useCallback(
    async (config: AppConfig) => {
      setError(null);
      try {
        await api.startRun(config);
      } catch (rejection) {
        if (mounted.current) setError(toRunError(rejection));
      }
    },
    [api],
  );

  // Stays latched after the first Stop until Rust publishes the next snapshot,
  // so a second click cannot reach the backend.
  const stop = useCallback(async () => {
    if (stopInFlight.current) return;
    stopInFlight.current = true;
    setStopPending(true);
    try {
      await api.stopRun();
    } catch (rejection) {
      stopInFlight.current = false;
      if (mounted.current) {
        setError(toRunError(rejection));
        setStopPending(false);
      }
    }
  }, [api]);

  const dismissError = useCallback(() => setError(null), []);

  return { snapshot, error, stopPending, start, stop, dismissError };
}
