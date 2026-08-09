import type { NaturalOverrides, Preset } from "../domain/config";
import type { FieldErrors } from "../domain/validation";

interface ModeControlsProps {
  config: Preset;
  onChange: (preset: Preset) => void;
  disabled?: boolean;
  errors?: FieldErrors;
}

function interpolate(start: number, end: number, naturalness: number) {
  return Math.floor(
    (start * (100 - naturalness) + end * naturalness + 50) / 100,
  );
}

function naturalOverridesFromSlider(naturalness: number): NaturalOverrides {
  const pauseChanceBasisPoints = interpolate(100, 1_200, naturalness);
  return {
    minIntervalMs: interpolate(140, 55, naturalness),
    maxIntervalMs: interpolate(220, 480, naturalness),
    burstIntensity: interpolate(8, 100, naturalness),
    pauseChancePercent: Math.floor((pauseChanceBasisPoints + 50) / 100),
  };
}

function errorId(field: string) {
  return `mode-error-${field.replace(/[^a-z0-9]+/gi, "-")}`;
}

export function ModeControls({
  config,
  onChange,
  disabled = false,
  errors = {},
}: ModeControlsProps) {
  const describedBy = (...fields: string[]) => {
    const ids = fields.filter((field) => errors[field]).map(errorId);
    return ids.length > 0 ? ids.join(" ") : undefined;
  };
  const updateAdvanced = <Key extends keyof NaturalOverrides>(
    field: Key,
    value: NaturalOverrides[Key],
  ) => {
    onChange({
      ...config,
      natural: {
        ...config.natural,
        advanced: {
          ...(config.natural.advanced ??
            naturalOverridesFromSlider(config.natural.naturalness)),
          [field]: value,
        },
      },
    });
  };

  const advanced =
    config.natural.advanced ??
    naturalOverridesFromSlider(config.natural.naturalness);

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
            aria-describedby={describedBy("timer.intervalMs")}
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
              aria-describedby={describedBy("natural.naturalness")}
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
                  aria-describedby={describedBy(
                    "natural.advanced.minIntervalMs",
                    "natural.advanced",
                  )}
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
                  aria-describedby={describedBy(
                    "natural.advanced.maxIntervalMs",
                    "natural.advanced",
                  )}
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
                  aria-describedby={describedBy(
                    "natural.advanced.burstIntensity",
                  )}
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
                  aria-describedby={describedBy(
                    "natural.advanced.pauseChancePercent",
                  )}
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
          <p className="field-error" id={errorId(field)} key={field}>
            {message}
          </p>
        ))}
    </section>
  );
}
