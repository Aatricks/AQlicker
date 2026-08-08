import type { LogicalKey } from "../domain/config";
import { keyLabel } from "./KeySequenceEditor";

interface ErrorNoticeProps {
  code: string;
  failedKey?: LogicalKey | null;
  detail?: string | null;
  sameIntegrityOnly?: boolean;
  onDismiss?: () => void;
}

const MESSAGES: Record<string, string> = {
  "permission-required":
    "AQlicker needs input permission before it can start a run.",
  "shortcut-conflict":
    "That shortcut is already in use. Record a different combination.",
  "shortcut-unavailable":
    "The global shortcut is not registered, so the run did not begin.",
  "escape-unavailable":
    "Escape could not be reserved as the emergency stop, so the run did not begin.",
  "escape-cleanup-failed":
    "Escape is still reserved from the previous run. Try starting again.",
  "invalid-config":
    "Fix the highlighted settings before starting a run.",
  "worker-panic":
    "The run stopped unexpectedly and no key is left held down.",
  "corrupt-config-recovered":
    "Saved settings could not be read, so AQlicker kept the original file and loaded defaults.",
  "config-load-failed": "AQlicker could not read its saved settings.",
  "run-failed":
    "The last run ended in an unsafe state. Restart AQlicker before starting another run.",
  "run-busy": "A run is already active.",
  "service-unavailable": "AQlicker is not responding. Restart the application.",
  "service-shutting-down": "AQlicker is shutting down.",
};

const ELEVATED_HINT =
  " The focused application may be running at a higher privilege level; both applications must run at compatible levels.";

function describe({
  code,
  failedKey,
  detail,
  sameIntegrityOnly,
}: ErrorNoticeProps) {
  if (code === "input-failure") {
    const target = failedKey ? `the ${keyLabel(failedKey)} key` : "a key";
    const platform = detail ? ` (${detail})` : "";
    return `AQlicker could not send ${target} and stopped the run${platform}.${
      sameIntegrityOnly ? ELEVATED_HINT : ""
    }`;
  }
  return MESSAGES[code] ?? detail ?? code;
}

export function ErrorNotice(props: ErrorNoticeProps) {
  return (
    <div className="error-notice" role="alert">
      <p>{describe(props)}</p>
      {props.onDismiss && (
        <button onClick={props.onDismiss} type="button">
          Dismiss
        </button>
      )}
    </div>
  );
}
