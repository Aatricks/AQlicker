import type { AppConfig, NaturalOverrides } from "../domain/config";
import type { FieldErrors } from "../domain/validation";

interface ModeControlsProps {
  config: AppConfig;
  onChange: (config: AppConfig) => void;
  disabled?: boolean;
  errors?: FieldErrors;
}

const DEFAULT_ADVANCED: NaturalOverrides = {
  minIntervalMs: 80,
  maxIntervalMs: 450,
  burstIntensity: 40,
  pauseChancePercent: 5,
};

export function ModeControls({
  config,
  onChange,
  disabled = false,
  errors = {},
}: ModeControlsProps) {
  const updateAdvanced = <Key extends keyof NaturalOverrides>(
    field: Key,
    value: NaturalOverrides[Key],
  ) => {
    onChange({
      ...config,
      natural: {
        ...config.natural,
        advanced: {
          ...(config.natural.advanced ?? DEFAULT_ADVANCED),
          [field]: value,
        },
      },
    });
  };

  const advanced = config.natural.advanced ?? DEFAULT_ADVANCED;

  return (
    <section className="config-section mode-controls" aria-labelledby="mode-title">
      <div className="section-heading">
        <div>
          <h2 id="mode-title">Mode</h2>
          <p>Choose fixed timing or a more varied rhythm.</p>
        </div>
      </div>

      <div className="segmented-control" role="group" aria-label="Clicking mode">
        {(["timer", "natural"] as const).map((mode) => (
          <button
            aria-pressed={config.mode === mode}
            disabled={disabled}
            key={mode}
            onClick={() => onChange({ ...config, mode })}
            type="button"
          >
            {mode === "timer" ? "Timer" : "Natural"}
          </button>
        ))}
      </div>

      {config.mode === "timer" ? (
        <label className="field-row">
          <span>Timer interval (ms)</span>
          <input
            aria-invalid={Boolean(errors["timer.intervalMs"])}
            disabled={disabled}
            max={60_000}
            min={40}
            onChange={(event) =>
              onChange({
                ...config,
                timer: { intervalMs: Number(event.currentTarget.value) },
              })
            }
            type="number"
            value={config.timer.intervalMs}
          />
        </label>
      ) : (
        <div className="natural-controls">
          <label className="slider-field">
            <span>
              Naturalness <output>{config.natural.naturalness}</output>
            </span>
            <input
              aria-label="Naturalness"
              aria-invalid={Boolean(errors["natural.naturalness"])}
              disabled={disabled}
              max={100}
              min={0}
              onChange={(event) =>
                onChange({
                  ...config,
                  natural: {
                    naturalness: Number(event.currentTarget.value),
                    advanced: null,
                  },
                })
              }
              type="range"
              value={config.natural.naturalness}
            />
          </label>

          <details className="advanced-controls">
            <summary>Advanced</summary>
            <p>Fine-tune intervals, bursts, and pauses.</p>
            <div className="field-grid">
              <label>
                <span>Minimum interval (ms)</span>
                <input
                  aria-invalid={Boolean(
                    errors["natural.advanced.minIntervalMs"] ||
                      errors["natural.advanced"],
                  )}
                  disabled={disabled}
                  max={5_000}
                  min={40}
                  onChange={(event) =>
                    updateAdvanced(
                      "minIntervalMs",
                      Number(event.currentTarget.value),
                    )
                  }
                  type="number"
                  value={advanced.minIntervalMs}
                />
              </label>
              <label>
                <span>Maximum interval (ms)</span>
                <input
                  aria-invalid={Boolean(
                    errors["natural.advanced.maxIntervalMs"] ||
                      errors["natural.advanced"],
                  )}
                  disabled={disabled}
                  max={5_000}
                  min={40}
                  onChange={(event) =>
                    updateAdvanced(
                      "maxIntervalMs",
                      Number(event.currentTarget.value),
                    )
                  }
                  type="number"
                  value={advanced.maxIntervalMs}
                />
              </label>
              <label>
                <span>Burst intensity</span>
                <input
                  aria-invalid={Boolean(
                    errors["natural.advanced.burstIntensity"],
                  )}
                  disabled={disabled}
                  max={100}
                  min={0}
                  onChange={(event) =>
                    updateAdvanced(
                      "burstIntensity",
                      Number(event.currentTarget.value),
                    )
                  }
                  type="number"
                  value={advanced.burstIntensity}
                />
              </label>
              <label>
                <span>Pause chance (%)</span>
                <input
                  aria-invalid={Boolean(
                    errors["natural.advanced.pauseChancePercent"],
                  )}
                  disabled={disabled}
                  max={25}
                  min={0}
                  onChange={(event) =>
                    updateAdvanced(
                      "pauseChancePercent",
                      Number(event.currentTarget.value),
                    )
                  }
                  type="number"
                  value={advanced.pauseChancePercent}
                />
              </label>
            </div>
          </details>
        </div>
      )}

      {Object.entries(errors)
        .filter(([field]) =>
          field.startsWith(config.mode === "timer" ? "timer." : "natural."),
        )
        .map(([field, message]) => (
          <p className="field-error" key={field}>
            {message}
          </p>
        ))}
    </section>
  );
}
