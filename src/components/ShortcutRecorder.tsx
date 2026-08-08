import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { LOGICAL_KEYS, type LogicalKey } from "../domain/config";

interface ShortcutRecorderProps {
  value: string;
  onRecord: (candidate: string) => Promise<string>;
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
      aria-labelledby="shortcut-title"
    >
      <div className="section-heading">
        <div>
          <h2 id="shortcut-title">Global shortcut</h2>
          <p>Toggle AQlicker from any application.</p>
        </div>
      </div>

      <div className="shortcut-row">
        <kbd>{value}</kbd>
        {recording ? (
          <button
            aria-label={registering ? "Registering shortcut" : "Press shortcut"}
            className="recording-button"
            disabled={disabled || registering}
            onKeyDown={capture}
            ref={recorderRef}
            type="button"
          >
            {registering ? "Registering…" : "Press shortcut…"}
          </button>
        ) : (
          <button disabled={disabled} onClick={beginRecording} type="button">
            Record shortcut
          </button>
        )}
      </div>
      {recording && !registering && (
        <p className="capture-help">Hold a modifier, then press a supported key.</p>
      )}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
