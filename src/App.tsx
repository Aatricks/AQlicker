import { aqlickerApi, type AqlickerApi } from "./api/aqlicker";
import { KeySequenceEditor } from "./components/KeySequenceEditor";
import { ModeControls } from "./components/ModeControls";
import { ShortcutRecorder } from "./components/ShortcutRecorder";
import { StopAfterControls } from "./components/StopAfterControls";
import { useConfig } from "./hooks/useConfig";

interface AppProps {
  api?: AqlickerApi;
}

function App({ api = aqlickerApi }: AppProps) {
  const {
    config,
    errors,
    startErrors,
    loading,
    loadError,
    saveError,
    updateConfig,
    registerShortcut,
  } = useConfig(api);
  const bootstrapStatus = loading
    ? "Loading"
    : loadError
      ? "Unavailable"
      : "Ready";

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
            {bootstrapStatus}
          </p>
        </header>

        {loadError && (
          <p className="error-notice" role="alert">
            Could not load settings: {loadError}
          </p>
        )}

        {config && (
          <div className="configuration-stack">
            <KeySequenceEditor
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
              errors={errors}
              onChange={updateConfig}
            />
            <StopAfterControls
              error={errors.stopAfter}
              onChange={(stopAfter) =>
                updateConfig((current) => ({ ...current, stopAfter }))
              }
              value={config.stopAfter}
            />
            <ShortcutRecorder
              onRecord={registerShortcut}
              value={config.globalShortcut}
            />
          </div>
        )}

        {saveError && (
          <p className="error-notice" role="alert">
            Could not save settings: {saveError}
          </p>
        )}

        <footer className="run-footer">
          <div>
            <strong>{loadError ? "Unavailable" : "Idle"}</strong>
            <span>
              {loadError ? "Settings could not be loaded" : "Configure a run above"}
            </span>
          </div>
          <button className="start-button" type="button" disabled>
            Start
          </button>
        </footer>
      </div>
    </main>
  );
}

export default App;
