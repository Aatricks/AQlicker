import { useCallback, useEffect, useRef, useState } from "react";
import {
  aqlickerApi,
  type AqlickerApi,
  type PermissionStatus,
} from "./api/aqlicker";
import { ErrorNotice } from "./components/ErrorNotice";
import { KeySequenceEditor } from "./components/KeySequenceEditor";
import { ModeControls } from "./components/ModeControls";
import { PermissionBanner } from "./components/PermissionBanner";
import { PresetControls } from "./components/PresetControls";
import { RunControls } from "./components/RunControls";
import { ShortcutRecorder } from "./components/ShortcutRecorder";
import { StopAfterControls } from "./components/StopAfterControls";
import { TargetAppPicker } from "./components/TargetAppPicker";
import { activePreset } from "./domain/config";
import { useConfig } from "./hooks/useConfig";
import { useRunState } from "./hooks/useRunState";

interface AppProps {
  api?: AqlickerApi;
}

const UNKNOWN_PERMISSION: PermissionStatus = {
  granted: false,
  sameIntegrityOnly: false,
};

function App({ api = aqlickerApi }: AppProps) {
  const {
    bootstrap,
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
  } = useConfig(api);
  const run = useRunState(api, bootstrap?.run);

  const [permissionOverride, setPermissionOverride] =
    useState<PermissionStatus | null>(null);
  const [shortcutOverride, setShortcutOverride] = useState<boolean | null>(null);
  const [cycleShortcutOverride, setCycleShortcutOverride] = useState<
    boolean | null
  >(null);
  const [requesting, setRequesting] = useState(false);
  const [recoveryDismissed, setRecoveryDismissed] = useState(false);

  const permission =
    permissionOverride ?? bootstrap?.permission ?? UNKNOWN_PERMISSION;
  const shortcutRegistered =
    shortcutOverride ?? bootstrap?.shortcut.registered ?? false;

  useEffect(() => {
    let active = true;
    const refresh = () => {
      void api
        .permissionStatus()
        .then((status) => {
          if (active) setPermissionOverride(status);
        })
        .catch(() => undefined);
    };

    window.addEventListener("focus", refresh);
    return () => {
      active = false;
      window.removeEventListener("focus", refresh);
    };
  }, [api]);

  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const requestAccess = useCallback(() => {
    setRequesting(true);
    void api
      .requestAccess()
      .then((status) => {
        if (mounted.current) setPermissionOverride(status);
      })
      .catch(() => undefined)
      .finally(() => {
        if (mounted.current) setRequesting(false);
      });
  }, [api]);

  const recordShortcut = useCallback(
    async (candidate: string) => {
      const registered = await registerShortcut(candidate);
      setShortcutOverride(true);
      return registered;
    },
    [registerShortcut],
  );

  const recordCycleShortcut = useCallback(
    async (candidate: string) => {
      const registered = await registerCycleShortcut(candidate);
      setCycleShortcutOverride(true);
      return registered;
    },
    [registerCycleShortcut],
  );

  const clearCycleShortcut = useCallback(async () => {
    const registered = await registerCycleShortcut(null);
    setCycleShortcutOverride(true);
    return registered;
  }, [registerCycleShortcut]);

  /**
   * A cycling shortcut another application already owns would otherwise look
   * exactly like a working one and simply do nothing.
   */
  const cycleShortcutRegistered =
    cycleShortcutOverride ?? bootstrap?.cycleShortcut?.registered ?? true;

  const listApps = useCallback(() => api.listApps(), [api]);

  const locked =
    run.snapshot.status === "running" || run.snapshot.status === "stopping";
  const headerStatus = locked
    ? run.snapshot.paused
      ? `Paused · waiting for ${run.snapshot.waitingForApp ?? "target"}`
      : `${run.snapshot.status === "stopping" ? "Stopping" : "Running"} · ${
          run.snapshot.mode === "natural" ? "Natural" : "Timer"
        }`
    : loading
      ? "Loading"
      : loadError
        ? "Unavailable"
        : "Ready";

  const preset = config ? activePreset(config) : null;

  const blockers = config
    ? Array.from(
        new Set([
          ...Object.values(startErrors),
          ...(permission.granted ? [] : ["Grant input permission"]),
          ...(shortcutRegistered ? [] : ["Register the global shortcut"]),
        ]),
      )
    : [loadError ? "Settings could not be loaded" : "Loading settings"];

  const recoveryCode =
    !recoveryDismissed && bootstrap?.recoveryNotice
      ? bootstrap.recoveryNotice.code
      : null;

  return (
    <main className="app-background">
      <div className="app-shell">
        <header className="app-header">
          <div>
            <p className="eyebrow">Desktop key repeater</p>
            <h1>AQlicker</h1>
          </div>
          <p
            className={`status${loadError ? " status-error" : ""}`}
            role="status"
          >
            <span aria-hidden="true" />
            {headerStatus}
          </p>
        </header>

        {loadError && (
          <p className="error-notice" role="alert">
            Could not load settings: {loadError}
          </p>
        )}

        {recoveryCode && (
          <ErrorNotice
            code={recoveryCode}
            onDismiss={() => setRecoveryDismissed(true)}
          />
        )}

        {bootstrap && (
          <PermissionBanner
            onRequestAccess={requestAccess}
            requesting={requesting}
            status={permission}
          />
        )}

        {config && preset && (
          <div className="configuration-stack">
            <PresetControls
              config={config}
              disabled={locked}
              errors={errors}
              onChange={updateConfig}
            />
            <KeySequenceEditor
              disabled={locked}
              error={startErrors.keys}
              errors={errors}
              mode={preset.mode}
              onChange={(keys) =>
                updatePreset((current) => ({ ...current, keys }))
              }
              value={preset.keys}
            />
            <ModeControls
              config={preset}
              disabled={locked}
              errors={errors}
              onChange={(next) => updatePreset(() => next)}
            />
            <TargetAppPicker
              disabled={locked}
              listApps={listApps}
              onChange={(targetApp) =>
                updatePreset((current) => ({ ...current, targetApp }))
              }
              value={preset.targetApp}
            />
            <StopAfterControls
              disabled={locked}
              error={errors.stopAfter}
              onChange={(stopAfter) =>
                updatePreset((current) => ({ ...current, stopAfter }))
              }
              value={preset.stopAfter}
            />
            <ShortcutRecorder
              disabled={locked}
              onRecord={recordShortcut}
              value={config.globalShortcut}
            />
            <ShortcutRecorder
              description="Switch to the next preset from any application. Refused while a run is active."
              disabled={locked}
              id="cycle-shortcut"
              label="preset cycling shortcut"
              onClear={clearCycleShortcut}
              onRecord={recordCycleShortcut}
              title="Preset cycling shortcut"
              value={config.presetCycleShortcut}
              warning={
                config.presetCycleShortcut !== null && !cycleShortcutRegistered
                  ? "Another application already uses it. Record another one."
                  : null
              }
            />
          </div>
        )}

        {saveError && (
          <p className="error-notice" role="alert">
            Could not save settings: {saveError}
          </p>
        )}

        {run.error && (
          <ErrorNotice
            code={run.error.code}
            detail={run.error.message}
            failedKey={run.error.key}
            onDismiss={run.dismissError}
            sameIntegrityOnly={permission.sameIntegrityOnly}
          />
        )}

        <RunControls
          blockers={blockers}
          onStart={() => {
            if (config) void run.start(config);
          }}
          onStop={() => void run.stop()}
          snapshot={run.snapshot}
          stopPending={run.stopPending}
        />
      </div>
    </main>
  );
}

export default App;
