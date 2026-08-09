import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { LOGICAL_KEYS, type LogicalKey } from "../domain/config";

interface ShortcutRecorderProps {
  /** `null` renders as unassigned, which the cycling shortcut starts as. */
  value: string | null;
  onRecord: (candidate: string) => Promise<unknown>;
  /** Omitted for a shortcut that must always be assigned. */
  onClear?: () => Promise<unknown>;
  /** Shown until the next successful recording clears it. */
  warning?: string | null;
  /** Also the accessible name every control here is built from, so two
   * recorders on one panel never share a name. */
  label?: string;
  id?: string;
  title?: string;
  description?: string;
  disabled?: boolean;
  platform?: string;
}

const MODIFIER_CODES = new Set([
  "AltLeft",
  "AltRight",
  "ControlLeft",
  "ControlRight",
  "MetaLeft",
  "MetaRight",
  "ShiftLeft",
  "ShiftRight",
]);

function isLogicalKey(code: string): code is LogicalKey {
  return (LOGICAL_KEYS as readonly string[]).includes(code);
}

function acceleratorKey(code: LogicalKey) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

function shortcutCandidate(
  event: KeyboardEvent<HTMLButtonElement>,
  platform: string,
) {
  const isMac = /Mac|iPhone|iPad|iPod/i.test(platform);
  const modifiers: string[] = [];

  if ((isMac && event.metaKey) || (!isMac && event.ctrlKey)) {
    modifiers.push("CommandOrControl");
  }
  if (isMac && event.ctrlKey) modifiers.push("Control");
  if (!isMac && event.metaKey) modifiers.push("Super");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");

  return modifiers.length > 0 && isLogicalKey(event.code)
    ? [...modifiers, acceleratorKey(event.code)].join("+")
    : null;
}

export function ShortcutRecorder({
  value,
  onRecord,
  onClear,
  warning = null,
  label = "global shortcut",
  id = "shortcut",
  title = "Global shortcut",
  description = "Toggle AQlicker from any application.",
  disabled = false,
  platform = typeof navigator === "undefined" ? "" : navigator.platform,
}: ShortcutRecorderProps) {
  const [recording, setRecording] = useState(false);
  const [registering, setRegistering] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recorderRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (recording && !registering) recorderRef.current?.focus();
  }, [recording, registering]);

  const beginRecording = () => {
    setError(null);
    setRecording(true);
  };

  const clear = () => {
    if (disabled || registering || !onClear) return;
    setError(null);
    setRegistering(true);
    void onClear()
      .catch(() => {
        setError("That shortcut could not be released. Try again.");
      })
      .finally(() => setRegistering(false));
  };

  const capture = (event: KeyboardEvent<HTMLButtonElement>) => {
    event.preventDefault();
    if (registering || event.repeat) return;

    if (event.code === "Escape") {
      setRecording(false);
      setError("Escape is reserved for emergency stop.");
      return;
    }
    if (MODIFIER_CODES.has(event.code)) return;
    if (!isLogicalKey(event.code)) {
      setError("Choose a supported non-modifier key.");
      return;
    }

    const candidate = shortcutCandidate(event, platform);
    if (!candidate) {
      setError("Include at least one modifier.");
      return;
    }

    setError(null);
    setRegistering(true);
    void onRecord(candidate)
      .then(() => {
        setRecording(false);
      })
      .catch(() => {
        setError("That shortcut could not be registered. Try another one.");
      })
      .finally(() => setRegistering(false));
  };

  return (
    <section
      className="config-section shortcut-recorder"
      aria-labelledby={`${id}-title`}
    >
      <div className="section-heading">
        <div>
          <h2 id={`${id}-title`}>{title}</h2>
          <p>{description}</p>
        </div>
      </div>

      <div className="shortcut-row">
        {value === null ? (
          <span className="shortcut-unset" id={`${id}-value`}>
            Not set
          </span>
        ) : (
          <kbd id={`${id}-value`}>{value}</kbd>
        )}
        {recording ? (
          <button
            aria-label={
              registering ? `Registering ${label}` : `Press ${label}`
            }
            className="recording-button"
            disabled={disabled || registering}
            onKeyDown={capture}
            ref={recorderRef}
            type="button"
          >
            {registering ? "Registering…" : "Press shortcut…"}
          </button>
        ) : (
          <button
            aria-describedby={`${id}-value`}
            aria-label={`Record ${label}`}
            disabled={disabled}
            onClick={beginRecording}
            type="button"
          >
            Record shortcut
          </button>
        )}
        {onClear && value !== null && !recording && (
          <button
            aria-describedby={`${id}-value`}
            aria-label={`Clear ${label}`}
            disabled={disabled || registering}
            onClick={clear}
            type="button"
          >
            Clear
          </button>
        )}
      </div>
      {recording && !registering && (
        <p className="capture-help">Hold a modifier, then press a supported key.</p>
      )}
      {(error ?? warning) && (
        <p className="field-error" role="alert">
          {error ?? warning}
        </p>
      )}
    </section>
  );
}
