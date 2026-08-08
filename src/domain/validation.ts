import type { AppConfig } from "./config";

export type FieldErrors = Record<string, string>;

function isWholeNumberInRange(value: number, minimum: number, maximum: number) {
  return Number.isInteger(value) && value >= minimum && value <= maximum;
}

export function validateConfig(config: AppConfig): FieldErrors {
  const errors: FieldErrors = {};

  if (
    new Set(config.keys.map(({ key }) => key)).size !== config.keys.length
  ) {
    errors.keys = "Each key can appear only once";
  }

  config.keys.forEach(({ weight, cooldownMs }, index) => {
    if (!isWholeNumberInRange(weight, 1, 10)) {
      errors[`keys[${index}].weight`] = "Choose a weight from 1 to 10";
    }
    if (!isWholeNumberInRange(cooldownMs, 0, 60_000)) {
      errors[`keys[${index}].cooldownMs`] =
        "Choose a cooldown from 0 to 60,000 ms";
    }
  });

  if (!isWholeNumberInRange(config.timer.intervalMs, 40, 60_000)) {
    errors["timer.intervalMs"] =
      "Choose an interval from 40 to 60,000 ms";
  }

  if (!isWholeNumberInRange(config.natural.naturalness, 0, 100)) {
    errors["natural.naturalness"] =
      "Choose a naturalness from 0 to 100";
  }

  const advanced = config.natural.advanced;
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
    config.stopAfter !== null &&
    !isWholeNumberInRange(config.stopAfter, 1, 86_400)
  ) {
    errors.stopAfter = "Choose a duration from 1 second to 24 hours";
  }

  return errors;
}

export function validateConfigForStart(config: AppConfig): FieldErrors {
  const errors = validateConfig(config);
  if (config.keys.length === 0) {
    errors.keys = "Choose at least one key";
  }
  return errors;
}
