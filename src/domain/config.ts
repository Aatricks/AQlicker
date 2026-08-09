export const LOGICAL_KEYS = [
  "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI",
  "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR",
  "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
  "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7",
  "Digit8", "Digit9", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9",
  "F10", "F11", "F12", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Space",
  "Enter", "Tab", "Backquote", "Minus", "Equal", "BracketLeft", "BracketRight",
  "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash",
] as const;

export type LogicalKey = (typeof LOGICAL_KEYS)[number];
export type Mode = "timer" | "natural";

const MAX_PAUSE_CHANCE_PERCENT = 25;
const MAX_COOLDOWN_MS = 60_000;

export interface NaturalOverrides {
  minIntervalMs: number;
  maxIntervalMs: number;
  burstIntensity: number;
  pauseChancePercent: number;
}

export interface TargetApp {
  /** Stable platform identifier: bundle id on macOS, executable name on Windows. */
  id: string;
  name: string;
}

export interface Preset {
  /** Opaque, generated in the webview. Identity lives here, not in the name. */
  id: string;
  name: string;
  /** `cooldownMs` is Natural mode only: 0 disables the cooldown. */
  keys: Array<{ key: LogicalKey; weight: number; cooldownMs: number }>;
  mode: Mode;
  timer: { intervalMs: number };
  natural: { naturalness: number; advanced: NaturalOverrides | null };
  stopAfter: number | null;
  targetApp: TargetApp | null;
}

export interface AppConfig {
  schemaVersion: 5;
  /** App-level, never per-preset, so switching presets never changes it. */
  globalShortcut: string;
  /**
   * App-level too. `null` leaves preset cycling unbound, which is where every
   * migrated and every fresh configuration starts.
   */
  presetCycleShortcut: string | null;
  activePresetId: string;
  presets: Preset[];
}

export interface ValidationError {
  field: string;
  code: string;
}

export const DEFAULT_PRESET_ID = "default";
export const DEFAULT_PRESET_NAME = "Default";
export const MAX_PRESET_NAME_LENGTH = 60;

export const DEFAULT_PRESET: Preset = {
  id: DEFAULT_PRESET_ID,
  name: DEFAULT_PRESET_NAME,
  keys: [],
  mode: "timer",
  timer: { intervalMs: 100 },
  natural: { naturalness: 50, advanced: null },
  stopAfter: null,
  targetApp: null,
};

export const DEFAULT_CONFIG: AppConfig = {
  schemaVersion: 5,
  globalShortcut: "CommandOrControl+Shift+K",
  presetCycleShortcut: null,
  activePresetId: DEFAULT_PRESET_ID,
  presets: [DEFAULT_PRESET],
};

export function activePreset(config: AppConfig): Preset | null {
  return (
    config.presets.find((preset) => preset.id === config.activePresetId) ?? null
  );
}

/**
 * The preset one step after the active one, wrapping around. `null` when the
 * active preset is unresolvable or is the only one, which makes cycling a
 * no-op. Mirrors `AppConfig::next_preset_id` in Rust.
 */
export function nextPresetId(config: AppConfig): string | null {
  const index = config.presets.findIndex(
    (preset) => preset.id === config.activePresetId,
  );
  if (index < 0 || config.presets.length < 2) return null;
  return config.presets[(index + 1) % config.presets.length].id;
}

/** Field paths are relative to the preset; `validateConfig` prefixes them. */
export function validatePreset(preset: Preset): ValidationError[] {
  const errors: ValidationError[] = [];

  if (preset.name.trim() === "") {
    errors.push({ field: "name", code: "required" });
  }
  if ([...preset.name.trim()].length > MAX_PRESET_NAME_LENGTH) {
    errors.push({ field: "name", code: "range" });
  }

  const seen = new Set<LogicalKey>();
  for (const entry of preset.keys) {
    if (seen.has(entry.key)) {
      errors.push({ field: "keys", code: "duplicate" });
      break;
    }
    seen.add(entry.key);
  }

  preset.keys.forEach((entry, index) => {
    if (!Number.isInteger(entry.weight) || entry.weight < 1 || entry.weight > 10) {
      errors.push({ field: `keys[${index}].weight`, code: "range" });
    }
    if (!Number.isInteger(entry.cooldownMs) || entry.cooldownMs < 0 || entry.cooldownMs > MAX_COOLDOWN_MS) {
      errors.push({ field: `keys[${index}].cooldownMs`, code: "range" });
    }
  });

  if (!Number.isInteger(preset.timer.intervalMs) || preset.timer.intervalMs < 40 || preset.timer.intervalMs > 60_000) {
    errors.push({ field: "timer.intervalMs", code: "range" });
  }

  if (!Number.isInteger(preset.natural.naturalness) || preset.natural.naturalness < 0 || preset.natural.naturalness > 100) {
    errors.push({ field: "natural.naturalness", code: "range" });
  }

  const advanced = preset.natural.advanced;
  if (advanced) {
    if (!Number.isInteger(advanced.minIntervalMs) || advanced.minIntervalMs < 40 || advanced.minIntervalMs > 5_000) {
      errors.push({ field: "natural.advanced.minIntervalMs", code: "range" });
    }
    if (!Number.isInteger(advanced.maxIntervalMs) || advanced.maxIntervalMs < 40 || advanced.maxIntervalMs > 5_000) {
      errors.push({ field: "natural.advanced.maxIntervalMs", code: "range" });
    }
    if (advanced.minIntervalMs > advanced.maxIntervalMs) {
      errors.push({ field: "natural.advanced", code: "ordering" });
    }
    if (!Number.isInteger(advanced.burstIntensity) || advanced.burstIntensity < 0 || advanced.burstIntensity > 100) {
      errors.push({ field: "natural.advanced.burstIntensity", code: "range" });
    }
    if (!Number.isInteger(advanced.pauseChancePercent) || advanced.pauseChancePercent < 0 || advanced.pauseChancePercent > MAX_PAUSE_CHANCE_PERCENT) {
      errors.push({ field: "natural.advanced.pauseChancePercent", code: "range" });
    }
  }

  if (preset.stopAfter !== null && (!Number.isInteger(preset.stopAfter) || preset.stopAfter < 1 || preset.stopAfter > 86_400)) {
    errors.push({ field: "stopAfter", code: "range" });
  }

  if (preset.targetApp !== null && preset.targetApp.id.trim() === "") {
    errors.push({ field: "targetApp.id", code: "required" });
  }

  return errors;
}

export function validateConfig(config: AppConfig): ValidationError[] {
  const errors: ValidationError[] = [];

  if (config.presets.length === 0) {
    errors.push({ field: "presets", code: "required" });
  }

  const seen = new Set<string>();
  for (const preset of config.presets) {
    if (seen.has(preset.id)) {
      errors.push({ field: "presets", code: "duplicate" });
      break;
    }
    seen.add(preset.id);
  }

  config.presets.forEach((preset, index) => {
    if (preset.id.trim() === "") {
      errors.push({ field: `presets[${index}].id`, code: "required" });
    }
    for (const error of validatePreset(preset)) {
      errors.push({ field: `presets[${index}].${error.field}`, code: error.code });
    }
  });

  if (activePreset(config) === null) {
    errors.push({ field: "activePresetId", code: "unknown" });
  }

  return errors;
}

export function validateConfigForStart(config: AppConfig): ValidationError[] {
  const errors = validateConfig(config);
  const index = config.presets.findIndex(
    (preset) => preset.id === config.activePresetId,
  );
  if (index >= 0 && config.presets[index].keys.length === 0) {
    errors.push({ field: `presets[${index}].keys`, code: "required" });
  }
  return errors;
}

export function serializeConfig(config: AppConfig): string {
  return JSON.stringify(config);
}
