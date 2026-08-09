import {
  activePreset,
  MAX_PRESET_NAME_LENGTH,
  type AppConfig,
  type Preset,
} from "./config";

export type FieldErrors = Record<string, string>;

function isWholeNumberInRange(value: number, minimum: number, maximum: number) {
  return Number.isInteger(value) && value >= minimum && value <= maximum;
}

function presetNameError(name: string) {
  if (name.trim() === "") return "Name the preset";
  if ([...name.trim()].length > MAX_PRESET_NAME_LENGTH) {
    return `Keep the name to ${MAX_PRESET_NAME_LENGTH} characters`;
  }
  return null;
}

function presetLabel(preset: Preset, index: number) {
  const name = preset.name.trim();
  return name === "" ? `Preset ${index + 1}` : `Preset "${name}"`;
}

/** Field paths are relative to the preset; `validateConfig` prefixes them. */
function validatePreset(preset: Preset): FieldErrors {
  const errors: FieldErrors = {};

  const name = presetNameError(preset.name);
  if (name) errors.name = name;

  if (new Set(preset.keys.map(({ key }) => key)).size !== preset.keys.length) {
    errors.keys = "Each key can appear only once";
  }

  preset.keys.forEach(({ weight, cooldownMs }, index) => {
    if (!isWholeNumberInRange(weight, 1, 10)) {
      errors[`keys[${index}].weight`] = "Choose a weight from 1 to 10";
    }
    if (!isWholeNumberInRange(cooldownMs, 0, 60_000)) {
      errors[`keys[${index}].cooldownMs`] =
        "Choose a cooldown from 0 to 60,000 ms";
    }
  });

  if (!isWholeNumberInRange(preset.timer.intervalMs, 40, 60_000)) {
    errors["timer.intervalMs"] = "Choose an interval from 40 to 60,000 ms";
  }

  if (!isWholeNumberInRange(preset.natural.naturalness, 0, 100)) {
    errors["natural.naturalness"] = "Choose a naturalness from 0 to 100";
  }

  const advanced = preset.natural.advanced;
  if (advanced) {
    if (!isWholeNumberInRange(advanced.minIntervalMs, 40, 5_000)) {
      errors["natural.advanced.minIntervalMs"] =
        "Choose a minimum interval from 40 to 5,000 ms";
    }
    if (!isWholeNumberInRange(advanced.maxIntervalMs, 40, 5_000)) {
      errors["natural.advanced.maxIntervalMs"] =
        "Choose a maximum interval from 40 to 5,000 ms";
    }
    if (advanced.minIntervalMs > advanced.maxIntervalMs) {
      errors["natural.advanced"] =
        "Minimum interval cannot exceed maximum interval";
    }
    if (!isWholeNumberInRange(advanced.burstIntensity, 0, 100)) {
      errors["natural.advanced.burstIntensity"] =
        "Choose a burst intensity from 0 to 100";
    }
    if (!isWholeNumberInRange(advanced.pauseChancePercent, 0, 25)) {
      errors["natural.advanced.pauseChancePercent"] =
        "Choose a pause chance from 0 to 25%";
    }
  }

  if (
    preset.stopAfter !== null &&
    !isWholeNumberInRange(preset.stopAfter, 1, 86_400)
  ) {
    errors.stopAfter = "Choose a duration from 1 second to 24 hours";
  }

  if (preset.targetApp !== null && preset.targetApp.id.trim() === "") {
    errors["targetApp.id"] = "Choose a target application or none";
  }

  return errors;
}

/**
 * Covers exactly what `AppConfig::validate` rejects in Rust. Anything it misses
 * would pass this gate, be refused by the backend, and strand every later edit
 * in React state with nothing on screen to fix.
 *
 * The active preset's messages stay unprefixed because its controls are on
 * screen. Every other preset's messages are prefixed and name the preset, since
 * the field they belong to is not visible.
 */
export function validateConfig(config: AppConfig): FieldErrors {
  const errors: FieldErrors = {};

  if (config.presets.length === 0) {
    errors.presets = "Keep at least one preset";
  }

  const seen = new Set<string>();
  for (const preset of config.presets) {
    if (seen.has(preset.id)) {
      errors.presets = "Two presets share an identifier";
      break;
    }
    seen.add(preset.id);
  }

  config.presets.forEach((preset, index) => {
    if (preset.id.trim() === "") {
      errors[`presets[${index}].id`] =
        `${presetLabel(preset, index)}: has no identifier`;
    }
    const isActive = preset.id === config.activePresetId;
    for (const [field, message] of Object.entries(validatePreset(preset))) {
      if (isActive) {
        errors[field] = message;
      } else {
        errors[`presets[${index}].${field}`] =
          `${presetLabel(preset, index)}: ${message}`;
      }
    }
  });

  if (activePreset(config) === null) {
    errors.activePresetId = "Choose a preset";
  }

  return errors;
}

export function validateConfigForStart(config: AppConfig): FieldErrors {
  const errors = validateConfig(config);
  const active = activePreset(config);
  if (active && active.keys.length === 0) {
    errors.keys = "Choose at least one key";
  }
  return errors;
}
