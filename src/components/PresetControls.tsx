import { useState } from "react";
import {
  MAX_PRESET_NAME_LENGTH,
  type AppConfig,
  type Preset,
} from "../domain/config";
import type { FieldErrors } from "../domain/validation";

interface PresetControlsProps {
  config: AppConfig;
  disabled: boolean;
  errors?: FieldErrors;
  onChange: (config: AppConfig) => void;
}

const DELETE_LAST_REASON = "The last preset cannot be deleted.";

function trimToLimit(name: string) {
  return [...name.trim()].slice(0, MAX_PRESET_NAME_LENGTH).join("");
}

function copyOf(preset: Preset, presets: Preset[]) {
  const taken = new Set(presets.map(({ name }) => name));
  const base = trimToLimit(`${preset.name} copy`);
  if (!taken.has(base)) return base;
  for (let suffix = 2; ; suffix += 1) {
    const candidate = trimToLimit(`${preset.name} copy ${suffix}`);
    if (!taken.has(candidate)) return candidate;
  }
}

/**
 * The whole panel edits whichever preset is active, so this control sits at the
 * top of it. Every mutation goes through `mutate`, which refuses while a run
 * holds the configuration locked: gating only the buttons would leave the
 * select's onChange and the rename form live.
 */
export function PresetControls({
  config,
  disabled,
  errors = {},
  onChange,
}: PresetControlsProps) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [nameError, setNameError] = useState<string | null>(null);

  const active = config.presets.find(({ id }) => id === config.activePresetId);
  const activeIndex = config.presets.findIndex(
    ({ id }) => id === config.activePresetId,
  );
  const onlyOne = config.presets.length <= 1;
  const storedNameError =
    activeIndex >= 0 ? errors[`presets[${activeIndex}].name`] : undefined;

  const mutate = (next: AppConfig) => {
    if (disabled) return;
    onChange(next);
  };

  const select = (id: string) => {
    if (disabled) return;
    if (!config.presets.some((preset) => preset.id === id)) return;
    setRenaming(null);
    setNameError(null);
    mutate({ ...config, activePresetId: id });
  };

  const add = (preset: Preset) => {
    if (disabled) return;
    setRenaming(null);
    setNameError(null);
    mutate({
      ...config,
      activePresetId: preset.id,
      presets: [...config.presets, preset],
    });
  };

  const create = () => {
    if (disabled) return;
    add({
      id: crypto.randomUUID(),
      name: trimToLimit(`Preset ${config.presets.length + 1}`),
      keys: [],
      mode: "timer",
      timer: { intervalMs: 100 },
      natural: { naturalness: 50, advanced: null },
      stopAfter: null,
      targetApp: null,
    });
  };

  const duplicate = () => {
    if (disabled || !active) return;
    add({
      ...structuredClone(active),
      id: crypto.randomUUID(),
      name: copyOf(active, config.presets),
    });
  };

  const remove = () => {
    if (disabled || onlyOne || !active) return;
    const remaining = config.presets.filter(({ id }) => id !== active.id);
    setRenaming(null);
    setNameError(null);
    mutate({
      ...config,
      activePresetId: remaining[0].id,
      presets: remaining,
    });
  };

  const commitRename = () => {
    if (disabled || !active || renaming === null) return;
    const name = renaming.trim();
    if (name === "") {
      setNameError("Name the preset");
      return;
    }
    if ([...name].length > MAX_PRESET_NAME_LENGTH) {
      setNameError(`Keep the name to ${MAX_PRESET_NAME_LENGTH} characters`);
      return;
    }
    setNameError(null);
    setRenaming(null);
    mutate({
      ...config,
      presets: config.presets.map((preset) =>
        preset.id === active.id ? { ...preset, name } : preset,
      ),
    });
  };

  return (
    <section className="config-section" aria-labelledby="preset-title">
      <div className="section-heading">
        <div>
          <h2 id="preset-title">Preset</h2>
          <p id="preset-help">
            Everything below belongs to the selected preset. The global shortcut
            does not.
          </p>
        </div>
      </div>

      <label className="target-app-field">
        <span>Active preset</span>
        <select
          aria-describedby="preset-help"
          disabled={disabled}
          onChange={(event) => select(event.currentTarget.value)}
          value={active?.id ?? ""}
        >
          {config.presets.map((preset, index) => (
            <option key={preset.id} value={preset.id}>
              {preset.name || `Preset ${index + 1}`}
            </option>
          ))}
        </select>
      </label>

      {renaming !== null ? (
        <form
          className="preset-rename"
          onSubmit={(event) => {
            event.preventDefault();
            commitRename();
          }}
        >
          <label className="target-app-field">
            <span>Preset name</span>
            <input
              aria-describedby={nameError ? "preset-name-error" : undefined}
              aria-invalid={nameError ? "true" : undefined}
              autoFocus
              disabled={disabled}
              maxLength={MAX_PRESET_NAME_LENGTH}
              onChange={(event) => setRenaming(event.currentTarget.value)}
              type="text"
              value={renaming}
            />
          </label>
          <div className="preset-actions">
            <button disabled={disabled} type="submit">
              Save name
            </button>
            <button
              onClick={() => {
                setRenaming(null);
                setNameError(null);
              }}
              type="button"
            >
              Cancel
            </button>
          </div>
          {(nameError ?? storedNameError) && (
            <p className="field-error" id="preset-name-error" role="alert">
              {nameError ?? storedNameError}
            </p>
          )}
        </form>
      ) : (
        <div className="preset-actions">
          <button disabled={disabled} onClick={create} type="button">
            New preset
          </button>
          <button disabled={disabled} onClick={duplicate} type="button">
            Duplicate preset
          </button>
          <button
            disabled={disabled}
            onClick={() => {
              if (disabled || !active) return;
              setNameError(null);
              setRenaming(active.name);
            }}
            type="button"
          >
            Rename preset
          </button>
          <button
            aria-describedby={onlyOne ? "preset-delete-reason" : undefined}
            disabled={disabled || onlyOne}
            onClick={remove}
            type="button"
          >
            Delete preset
          </button>
        </div>
      )}

      {onlyOne && (
        <p className="section-note" id="preset-delete-reason">
          {DELETE_LAST_REASON}
        </p>
      )}

      {storedNameError && renaming === null && (
        <p className="field-error" role="alert">
          {storedNameError}
        </p>
      )}
    </section>
  );
}
