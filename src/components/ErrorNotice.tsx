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
  "run-terminal-pending":
    "The previous run is still finishing. Try again in a moment.",
  "input-unavailable":
    "AQlicker could not open a connection for sending keys on this system.",
  "shortcut-rollback-failed":
    "The global shortcut could not be restored. Record it again.",
  "worker-spawn-failed": "AQlicker could not start the run. Try again.",
  "wait-timeout": "AQlicker timed out waiting for the run to finish.",
  "config-save-failed": "AQlicker could not save the configuration.",
  "shortcut-invalid": "That shortcut is not a combination AQlicker can register.",
  "shortcut-reserved": "That shortcut is reserved. Record a different one.",
  "unsupported-schema":
    "The saved settings come from a newer version of AQlicker.",
  "start-failed": "AQlicker could not start the run.",
};

const GENERIC = "AQlicker could not complete that action.";

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
  // Coded rejections carry no message, so an unmapped code must not leak the
  // raw backend identifier into the interface.
  return MESSAGES[code] ?? (detail ? detail : GENERIC);
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
