import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aqlickerApi,
  type AqlickerApi,
  type BootstrapPayload,
} from "../api/aqlicker";
import type { AppConfig } from "../domain/config";
import { validateConfig } from "../domain/validation";

type ConfigUpdater = AppConfig | ((current: AppConfig) => AppConfig);

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

  useEffect(() => {
    if (!config || Object.keys(errors).length > 0) return;

    const serialized = JSON.stringify(config);
    if (serialized === persistedConfig.current) return;

    const timeout = window.setTimeout(() => {
      void api
        .saveConfig(config)
        .then(() => {
          persistedConfig.current = serialized;
          setSaveError(null);
        })
        .catch((error: unknown) => setSaveError(errorMessage(error)));
    }, 250);

    return () => window.clearTimeout(timeout);
  }, [api, config, errors]);

  const updateConfig = useCallback((updater: ConfigUpdater) => {
    setConfig((current) => {
      if (!current) return current;
      return typeof updater === "function" ? updater(current) : updater;
    });
  }, []);

  const registerShortcut = useCallback(
    async (candidate: string) => {
      const registered = await api.setShortcut(candidate);
      updateConfig((current) => ({
        ...current,
        globalShortcut: registered,
      }));
      return registered;
    },
    [api, updateConfig],
  );

  return {
    bootstrap: bootstrapPayload,
    config,
    errors,
    loading,
    loadError,
    saveError,
    updateConfig,
    registerShortcut,
  };
}
