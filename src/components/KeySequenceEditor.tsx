import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import {
  LOGICAL_KEYS,
  type AppConfig,
  type LogicalKey,
  type Mode,
} from "../domain/config";
import type { FieldErrors } from "../domain/validation";

type KeyEntry = AppConfig["keys"][number];

interface KeySequenceEditorProps {
  value: KeyEntry[];
  onChange: (value: KeyEntry[]) => void;
  mode: Mode;
  disabled?: boolean;
  error?: string;
  errors?: FieldErrors;
}

const PUNCTUATION_LABELS: Partial<Record<LogicalKey, string>> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

export function keyLabel(key: LogicalKey) {
  if (key.startsWith("Key")) return key.slice(3);
  if (key.startsWith("Digit")) return key.slice(5);
  if (key.startsWith("Arrow")) return `${key.slice(5)} arrow`;
  return PUNCTUATION_LABELS[key] ?? key;
}

function isLogicalKey(code: string): code is LogicalKey {
  return (LOGICAL_KEYS as readonly string[]).includes(code);
}

function reorder(value: KeyEntry[], from: number, to: number) {
  const next = [...value];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export function KeySequenceEditor({
  value,
  onChange,
  mode,
  disabled = false,
  error,
  errors = {},
}: KeySequenceEditorProps) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [captureArmed, setCaptureArmed] = useState(false);
  const [captureError, setCaptureError] = useState<string | null>(null);
  const draggedKey = useRef<LogicalKey | null>(null);
  const chipRefs = useRef(new Map<LogicalKey, HTMLLIElement>());
  const addKeyRef = useRef<HTMLButtonElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const captureRef = useRef<HTMLButtonElement>(null);

  const filteredKeys = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return LOGICAL_KEYS;
    return LOGICAL_KEYS.filter((key) =>
      `${keyLabel(key)} ${key}`.toLocaleLowerCase().includes(normalized),
    );
  }, [query]);

  const openPicker = () => {
    setQuery("");
    setCaptureArmed(false);
    setCaptureError(null);
    setPickerOpen(true);
  };

  const closePicker = () => {
    setPickerOpen(false);
    addKeyRef.current?.focus();
  };

  useEffect(() => {
    if (pickerOpen) captureRef.current?.focus();
  }, [pickerOpen]);

  // A run can start from the global toggle while the dialog is open, and the
  // dialog cannot intercept an OS-level shortcut. Close it rather than let it
  // keep editing a configuration the rest of the interface reports as locked.
  useEffect(() => {
    if (disabled) setPickerOpen(false);
  }, [disabled]);

  const selectKey = (key: LogicalKey) => {
    if (disabled) return;
    const existing = value.find(({ key: selected }) => selected === key);
    if (existing) {
      setPickerOpen(false);
      chipRefs.current.get(key)?.focus();
      return;
    }
    onChange([...value, { key, weight: 1 }]);
    closePicker();
  };

  const capturePhysicalKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!captureArmed || disabled) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.code === "Escape") {
      closePicker();
      return;
    }
    if (!isLogicalKey(event.code)) {
      setCaptureError("That physical key is not supported.");
      return;
    }
    selectKey(event.code);
  };

  const containDialogFocus = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.code === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closePicker();
      return;
    }
    if (event.code !== "Tab") return;

    const focusable = Array.from(
      pickerRef.current?.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])',
      ) ?? [],
    );
    if (focusable.length === 0) return;

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const move = (index: number, offset: -1 | 1) => {
    const destination = index + offset;
    if (destination < 0 || destination >= value.length) return;
    onChange(reorder(value, index, destination));
  };

  const dropOn = (event: DragEvent<HTMLLIElement>, targetKey: LogicalKey) => {
    event.preventDefault();
    const sourceKey = draggedKey.current;
    draggedKey.current = null;
    if (!sourceKey || sourceKey === targetKey) return;
    const from = value.findIndex(({ key }) => key === sourceKey);
    const to = value.findIndex(({ key }) => key === targetKey);
    if (from >= 0 && to >= 0) onChange(reorder(value, from, to));
  };

  return (
    <section className="config-section key-editor" aria-labelledby="keys-title">
      <div className="section-heading">
        <div>
          <h2 id="keys-title">Key sequence</h2>
          <p>Keys repeat in this order.</p>
        </div>
        <button
          type="button"
          onClick={openPicker}
          disabled={disabled}
          ref={addKeyRef}
        >
          Add key
        </button>
      </div>

      {value.length === 0 ? (
        <p className="empty-state">Choose at least one physical key.</p>
      ) : (
        <ol className="key-list" aria-label="Selected keys">
          {value.map((entry, index) => {
            const label = keyLabel(entry.key);
            const weightError = errors[`keys[${index}].weight`];
            return (
              <li
                className="key-card"
                data-testid={`key-${entry.key}`}
                draggable={!disabled}
                key={entry.key}
                onDragStart={() => {
                  draggedKey.current = entry.key;
                }}
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => dropOn(event, entry.key)}
                ref={(node) => {
                  if (node) chipRefs.current.set(entry.key, node);
                  else chipRefs.current.delete(entry.key);
                }}
                tabIndex={-1}
              >
                <span className="drag-handle" aria-hidden="true">
                  ⋮⋮
                </span>
                <span className="key-name">{label}</span>
                {mode === "natural" && (
                  <label className="weight-control">
                    <span>{label} frequency weight</span>
                    <input
                      aria-describedby={
                        weightError ? `weight-error-${entry.key}` : undefined
                      }
                      aria-invalid={Boolean(weightError)}
                      aria-label={`${label} frequency weight`}
                      disabled={disabled}
                      max={10}
                      min={1}
                      onChange={(event) => {
                        const next = [...value];
                        next[index] = {
                          ...entry,
                          weight: Number(event.currentTarget.value),
                        };
                        onChange(next);
                      }}
                      type="number"
                      value={entry.weight}
                    />
                    {weightError && (
                      <span
                        className="field-error"
                        id={`weight-error-${entry.key}`}
                      >
                        {weightError}
                      </span>
                    )}
                  </label>
                )}
                <div className="key-actions">
                  <button
                    aria-label={`Move ${label} left`}
                    disabled={disabled || index === 0}
                    onClick={() => move(index, -1)}
                    type="button"
                  >
                    <span aria-hidden="true">←</span>
                  </button>
                  <button
                    aria-label={`Move ${label} right`}
                    disabled={disabled || index === value.length - 1}
                    onClick={() => move(index, 1)}
                    type="button"
                  >
                    <span aria-hidden="true">→</span>
                  </button>
                  <button
                    aria-label={`Remove ${label}`}
                    disabled={disabled}
                    onClick={() =>
                      onChange(value.filter(({ key }) => key !== entry.key))
                    }
                    type="button"
                  >
                    <span aria-hidden="true">×</span>
                  </button>
                </div>
              </li>
            );
          })}
        </ol>
      )}

      {error && <p className="field-error">{error}</p>}

      {pickerOpen && (
        <div className="modal-backdrop">
          <div
            aria-describedby="key-picker-help"
            aria-labelledby="key-picker-title"
            aria-modal="true"
            className="key-picker"
            onKeyDown={containDialogFocus}
            ref={pickerRef}
            role="dialog"
            tabIndex={-1}
          >
            <div className="section-heading">
              <div>
                <h2 id="key-picker-title">Add a physical key</h2>
                <p id="key-picker-help">Press a key or search the supported list.</p>
              </div>
              <button
                aria-label="Close key picker"
                onClick={closePicker}
                type="button"
              >
                ×
              </button>
            </div>
            <button
              aria-label="Physical key capture"
              aria-pressed={captureArmed}
              className="key-capture-surface"
              onClick={() => {
                setCaptureArmed(true);
                setCaptureError(null);
                captureRef.current?.focus();
              }}
              onKeyDown={capturePhysicalKey}
              ref={captureRef}
              type="button"
            >
              {captureArmed
                ? "Press a supported key now"
                : "Start physical key capture"}
            </button>
            <input
              aria-label="Search keys"
              onChange={(event) => setQuery(event.currentTarget.value)}
              placeholder="Search letters, arrows, Space…"
              type="search"
              value={query}
            />
            {captureError && (
              <p className="field-error" role="alert">
                {captureError}
              </p>
            )}
            <ul className="key-catalogue" aria-label="Supported keys">
              {filteredKeys.map((key) => (
                <li key={key}>
                  <button
                    className="key-option"
                    onClick={() => selectKey(key)}
                    type="button"
                  >
                    <span>{keyLabel(key)}</span>
                    <small>{key}</small>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
    </section>
  );
}
