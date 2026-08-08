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

export interface AppConfig {
  schemaVersion: 3;
  /** `cooldownMs` is Natural mode only: 0 disables the cooldown. */
  keys: Array<{ key: LogicalKey; weight: number; cooldownMs: number }>;
  mode: Mode;
  timer: { intervalMs: number };
  natural: { naturalness: number; advanced: NaturalOverrides | null };
  stopAfter: number | null;
  globalShortcut: string;
  targetApp: TargetApp | null;
}

export interface ValidationError {
  field: string;
  code: string;
}

export const DEFAULT_CONFIG: AppConfig = {
  schemaVersion: 3,
  keys: [],
  mode: "timer",
  timer: { intervalMs: 100 },
  natural: { naturalness: 50, advanced: null },
  stopAfter: null,
  globalShortcut: "CommandOrControl+Shift+K",
  targetApp: null,
};

export function validateConfig(config: AppConfig): ValidationError[] {
  const errors: ValidationError[] = [];

  const seen = new Set<LogicalKey>();
  for (const entry of config.keys) {
    if (seen.has(entry.key)) {
      errors.push({ field: "keys", code: "duplicate" });
      break;
    }
    seen.add(entry.key);
  }

  config.keys.forEach((entry, index) => {
    if (!Number.isInteger(entry.weight) || entry.weight < 1 || entry.weight > 10) {
      errors.push({ field: `keys[${index}].weight`, code: "range" });
    }
    if (!Number.isInteger(entry.cooldownMs) || entry.cooldownMs < 0 || entry.cooldownMs > MAX_COOLDOWN_MS) {
      errors.push({ field: `keys[${index}].cooldownMs`, code: "range" });
    }
  });

  if (!Number.isInteger(config.timer.intervalMs) || config.timer.intervalMs < 40 || config.timer.intervalMs > 60_000) {
    errors.push({ field: "timer.intervalMs", code: "range" });
  }

  if (!Number.isInteger(config.natural.naturalness) || config.natural.naturalness < 0 || config.natural.naturalness > 100) {
    errors.push({ field: "natural.naturalness", code: "range" });
  }

  const advanced = config.natural.advanced;
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

  if (config.stopAfter !== null && (!Number.isInteger(config.stopAfter) || config.stopAfter < 1 || config.stopAfter > 86_400)) {
    errors.push({ field: "stopAfter", code: "range" });
  }

  if (config.targetApp !== null && config.targetApp.id.trim() === "") {
    errors.push({ field: "targetApp.id", code: "required" });
  }

  return errors;
}

export function validateConfigForStart(config: AppConfig): ValidationError[] {
  const errors = validateConfig(config);
  if (config.keys.length === 0) {
    errors.push({ field: "keys", code: "required" });
  }
  return errors;
}

export function serializeConfig(config: AppConfig): string {
  return JSON.stringify(config);
}
