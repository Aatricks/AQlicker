import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aqlickerApi,
  type AqlickerApi,
  type BootstrapPayload,
} from "../api/aqlicker";
import type { AppConfig, Preset } from "../domain/config";
import {
  validateConfig,
  validateConfigForStart,
} from "../domain/validation";

type ConfigUpdater = AppConfig | ((current: AppConfig) => AppConfig);

type QueuedSave = {
  config: AppConfig;
  serialized: string;
};

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error && "code" in error) {
    return String(error.code);
  }
  return "AQlicker could not save the configuration";
}

export function useConfig(api: AqlickerApi = aqlickerApi) {
  const [bootstrapPayload, setBootstrapPayload] =
    useState<BootstrapPayload | null>(null);
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const persistedConfig = useRef<string | null>(null);
  const queuedSave = useRef<QueuedSave | null>(null);
  const saveInFlight = useRef(false);
  const mounted = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      queuedSave.current = null;
    };
  }, []);

  useEffect(() => {
    let active = true;

    void api
      .bootstrap()
      .then((payload) => {
        if (!active) return;
        persistedConfig.current = JSON.stringify(payload.config);
        setBootstrapPayload(payload);
        setConfig(payload.config);
      })
      .catch((error: unknown) => {
        if (active) setLoadError(errorMessage(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [api]);

  const errors = useMemo(
    () => (config ? validateConfig(config) : {}),
    [config],
  );
  const startErrors = useMemo(
    () => (config ? validateConfigForStart(config) : {}),
    [config],
  );

  const drainSaves = useCallback(
    function drainSaves() {
      if (!mounted.current || saveInFlight.current) return;

      const candidate = queuedSave.current;
      if (!candidate) return;
      queuedSave.current = null;

      if (candidate.serialized === persistedConfig.current) {
        setSaveError(null);
        return;
      }

      saveInFlight.current = true;
      void api
        .saveConfig(candidate.config)
        .then(
          () => {
            if (!mounted.current) return;
            persistedConfig.current = candidate.serialized;
            setSaveError(null);
          },
          (error: unknown) => {
            if (!mounted.current) return;
            if (!queuedSave.current) {
              setSaveError(errorMessage(error));
            }
          },
        )
        .finally(() => {
          saveInFlight.current = false;
          if (mounted.current) drainSaves();
        });
    },
    [api],
  );

  useEffect(() => {
    if (!config || Object.keys(errors).length > 0) return;

    const serialized = JSON.stringify(config);
    if (
      !saveInFlight.current &&
      !queuedSave.current &&
      serialized === persistedConfig.current
    ) {
      setSaveError(null);
      return;
    }

    const timeout = window.setTimeout(() => {
      queuedSave.current = { config, serialized };
      drainSaves();
    }, 250);

    return () => window.clearTimeout(timeout);
  }, [config, drainSaves, errors]);

  const updateConfig = useCallback((updater: ConfigUpdater) => {
    setConfig((current) => {
      if (!current) return current;
      return typeof updater === "function" ? updater(current) : updater;
    });
  }, []);

  /**
   * Every preset edit rewrites the whole document, so a switch that lands while
   * a save is in flight cannot write one preset's contents into another's slot:
   * the queued value is always a complete, self-consistent document.
   */
  const updatePreset = useCallback(
    (updater: (preset: Preset) => Preset) => {
      updateConfig((current) => ({
        ...current,
        presets: current.presets.map((preset) =>
          preset.id === current.activePresetId ? updater(preset) : preset,
        ),
      }));
    },
    [updateConfig],
  );

  /**
   * Preset cycling and the menu bar item change the configuration in Rust. Only
   * those two app-level fields are taken from the event and merged onto the
   * current draft: the emitted document is whatever Rust last saved, so
   * replacing the draft with it would throw away an edit still inside the
   * debounce. The marker moves to the emitted document because that is what is
   * genuinely on disk, which leaves the existing effect to notice the merged
   * draft differs and persist the edit on its own.
   */
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;

    void api
      .listenConfig((next) => {
        if (!active) return;
        persistedConfig.current = JSON.stringify(next);
        setConfig((current) =>
          current === null
            ? next
            : {
                ...current,
                // A draft that already deleted that preset must not be left
                // pointing at it: nothing on screen could fix it.
                activePresetId: current.presets.some(
                  (preset) => preset.id === next.activePresetId,
                )
                  ? next.activePresetId
                  : current.activePresetId,
                presetCycleShortcut: next.presetCycleShortcut,
              },
        );
      })
      .then((stop) => {
        if (active) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);

    return () => {
      active = false;
      unlisten?.();
    };
  }, [api]);

  const registerCycleShortcut = useCallback(
    async (candidate: string | null) => {
      const registered = await api.setCycleShortcut(candidate);
      if (mounted.current) {
        updateConfig((current) => ({
          ...current,
          presetCycleShortcut: registered,
        }));
      }
      return registered;
    },
    [api, updateConfig],
  );

  const registerShortcut = useCallback(
    async (candidate: string) => {
      const registered = await api.setShortcut(candidate);
      if (mounted.current) {
        updateConfig((current) => ({
          ...current,
          globalShortcut: registered,
        }));
      }
      return registered;
    },
    [api, updateConfig],
  );

  return {
    bootstrap: bootstrapPayload,
    config,
    errors,
    startErrors,
    loading,
    loadError,
    saveError,
    updateConfig,
    updatePreset,
    registerShortcut,
    registerCycleShortcut,
  };
}
