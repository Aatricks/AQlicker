import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  aqlickerApi,
  type AqlickerApi,
  type BootstrapPayload,
} from "../api/aqlicker";
import type { AppConfig } from "../domain/config";
import {
  validateConfig,
  validateConfigForStart,
} from "../domain/validation";

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
  const mounted = useRef(false);
  const saveGeneration = useRef(0);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      saveGeneration.current += 1;
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

  useEffect(() => {
    const generation = ++saveGeneration.current;
    if (!config || Object.keys(errors).length > 0) return;

    const serialized = JSON.stringify(config);
    if (serialized === persistedConfig.current) return;

    const timeout = window.setTimeout(() => {
      void api
        .saveConfig(config)
        .then(() => {
          if (!mounted.current || generation !== saveGeneration.current) {
            return;
          }
          persistedConfig.current = serialized;
          setSaveError(null);
        })
        .catch((error: unknown) => {
          if (mounted.current && generation === saveGeneration.current) {
            setSaveError(errorMessage(error));
          }
        });
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
    registerShortcut,
  };
}
