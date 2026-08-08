interface StopAfterControlsProps {
  value: number | null;
  onChange: (seconds: number | null) => void;
  disabled?: boolean;
  error?: string;
}

const QUICK_DURATIONS = [
  { label: "5 minutes", seconds: 300 },
  { label: "15 minutes", seconds: 900 },
  { label: "30 minutes", seconds: 1_800 },
  { label: "1 hour", seconds: 3_600 },
] as const;

function wholeNonNegative(value: string) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(0, Math.trunc(parsed)) : 0;
}

export function StopAfterControls({
  value,
  onChange,
  disabled = false,
  error,
}: StopAfterControlsProps) {
  const enabled = value !== null;
  const total = Math.max(0, Math.trunc(value ?? 0));
  const hours = Math.floor(total / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  const seconds = total % 60;

  const updateParts = (next: {
    hours?: number;
    minutes?: number;
    seconds?: number;
  }) => {
    onChange(
      (next.hours ?? hours) * 3_600 +
        (next.minutes ?? minutes) * 60 +
        (next.seconds ?? seconds),
    );
  };

  return (
    <section className="config-section stop-controls" aria-labelledby="stop-title">
      <div className="section-heading">
        <div>
          <h2 id="stop-title">Automatic stop</h2>
          <p>Shared by Timer and Natural modes.</p>
        </div>
        <label className="toggle-control">
          <input
            checked={enabled}
            disabled={disabled}
            onChange={(event) =>
              onChange(event.currentTarget.checked ? 300 : null)
            }
            type="checkbox"
          />
          <span>Stop after</span>
        </label>
      </div>

      <div className="duration-fields" role="group" aria-label="Stop duration">
        <label>
          <span>Hours</span>
          <input
            aria-describedby={error ? "stop-after-error" : undefined}
            aria-invalid={Boolean(error)}
            disabled={disabled || !enabled}
            max={24}
            min={0}
            onChange={(event) =>
              updateParts({ hours: wholeNonNegative(event.currentTarget.value) })
            }
            type="number"
            value={hours}
          />
        </label>
        <label>
          <span>Minutes</span>
          <input
            aria-describedby={error ? "stop-after-error" : undefined}
            aria-invalid={Boolean(error)}
            disabled={disabled || !enabled}
            max={59}
            min={0}
            onChange={(event) =>
              updateParts({ minutes: wholeNonNegative(event.currentTarget.value) })
            }
            type="number"
            value={minutes}
          />
        </label>
        <label>
          <span>Seconds</span>
          <input
            aria-describedby={error ? "stop-after-error" : undefined}
            aria-invalid={Boolean(error)}
            disabled={disabled || !enabled}
            max={59}
            min={0}
            onChange={(event) =>
              updateParts({ seconds: wholeNonNegative(event.currentTarget.value) })
            }
            type="number"
            value={seconds}
          />
        </label>
      </div>

      <div className="quick-durations" aria-label="Quick durations">
        {QUICK_DURATIONS.map(({ label, seconds: quickSeconds }) => (
          <button
            disabled={disabled}
            key={quickSeconds}
            onClick={() => onChange(quickSeconds)}
            type="button"
          >
            {label}
          </button>
        ))}
      </div>

      {error && (
        <p className="field-error" id="stop-after-error">
          {error}
        </p>
      )}
    </section>
  );
}
