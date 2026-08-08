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
import { RunControls } from "./components/RunControls";
import { ShortcutRecorder } from "./components/ShortcutRecorder";
import { StopAfterControls } from "./components/StopAfterControls";
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
    registerShortcut,
  } = useConfig(api);
  const run = useRunState(api, bootstrap?.run);

  const [permissionOverride, setPermissionOverride] =
    useState<PermissionStatus | null>(null);
  const [shortcutOverride, setShortcutOverride] = useState<boolean | null>(null);
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

  const locked =
    run.snapshot.status === "running" || run.snapshot.status === "stopping";
  const headerStatus = locked
    ? `${run.snapshot.status === "stopping" ? "Stopping" : "Running"} · ${
        run.snapshot.mode === "natural" ? "Natural" : "Timer"
      }`
    : loading
      ? "Loading"
      : loadError
        ? "Unavailable"
        : "Ready";

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

        {config && (
          <div className="configuration-stack">
            <KeySequenceEditor
              disabled={locked}
              error={startErrors.keys}
              errors={errors}
              mode={config.mode}
              onChange={(keys) =>
                updateConfig((current) => ({ ...current, keys }))
              }
              value={config.keys}
            />
            <ModeControls
              config={config}
              disabled={locked}
              errors={errors}
              onChange={updateConfig}
            />
            <StopAfterControls
              disabled={locked}
              error={errors.stopAfter}
              onChange={(stopAfter) =>
                updateConfig((current) => ({ ...current, stopAfter }))
              }
              value={config.stopAfter}
            />
            <ShortcutRecorder
              disabled={locked}
              onRecord={recordShortcut}
              value={config.globalShortcut}
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
