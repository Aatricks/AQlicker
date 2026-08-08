import type { RunSnapshot } from "../api/aqlicker";

interface RunControlsProps {
  snapshot: RunSnapshot;
  blockers: string[];
  stopPending: boolean;
  onStart: () => void;
  onStop: () => void;
}

const MODE_LABELS = { timer: "Timer", natural: "Natural" } as const;

function pad(value: number) {
  return String(value).padStart(2, "0");
}

export function formatDuration(milliseconds: number) {
  const total = Math.max(0, Math.floor(milliseconds / 1_000));
  const seconds = total % 60;
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3_600);
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(seconds)}`
    : `${minutes}:${pad(seconds)}`;
}

export function RunControls({
  snapshot,
  blockers,
  stopPending,
  onStart,
  onStop,
}: RunControlsProps) {
  const active = snapshot.status === "running" || snapshot.status === "stopping";
  const mode = snapshot.mode ? MODE_LABELS[snapshot.mode] : "Run";
  const headline = active
    ? `${mode} mode ${snapshot.status}`
    : snapshot.status === "failed"
      ? "Run stopped"
      : "Idle";

  return (
    <footer className="run-footer">
      <div>
        <strong>{headline}</strong>
        {active ? (
          <span className="run-metrics">
            <span>{`${formatDuration(snapshot.elapsedMs)} elapsed`}</span>
            {snapshot.remainingMs !== null && (
              <span>{`${formatDuration(snapshot.remainingMs)} remaining`}</span>
            )}
            <span>
              {snapshot.successfulPresses === 1
                ? "1 press"
                : `${snapshot.successfulPresses} presses`}
            </span>
          </span>
        ) : blockers.length > 0 ? (
          <ul className="run-blockers" id="run-blockers">
            {blockers.map((blocker) => (
              <li key={blocker}>{blocker}</li>
            ))}
          </ul>
        ) : (
          <span>Ready to start</span>
        )}
      </div>

      {active ? (
        <button
          className="start-button stop-button"
          disabled={stopPending || snapshot.status === "stopping"}
          onClick={onStop}
          type="button"
        >
          Stop
        </button>
      ) : (
        <button
          aria-describedby={blockers.length > 0 ? "run-blockers" : undefined}
          className="start-button"
          disabled={blockers.length > 0}
          onClick={onStart}
          type="button"
        >
          Start
        </button>
      )}
    </footer>
  );
}
