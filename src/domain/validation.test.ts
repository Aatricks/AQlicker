import { describe, expect, it } from "vitest";
import { DEFAULT_CONFIG, type AppConfig } from "./config";
import { validateConfig } from "./validation";

function configWith(overrides: Partial<AppConfig>): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    keys: [{ key: "KeyA", weight: 1 }],
    ...overrides,
  };
}

describe("configuration draft validation", () => {
  it("reports required, duplicate, duration, and mode-specific field errors", () => {
    expect(validateConfig(DEFAULT_CONFIG)).toMatchObject({
      keys: "Choose at least one key",
    });

    const invalid = configWith({
      keys: [
        { key: "KeyA", weight: 0 },
        { key: "KeyA", weight: 1 },
      ],
      timer: { intervalMs: 60_001 },
      stopAfter: 86_401,
    });

    expect(validateConfig(invalid)).toMatchObject({
      keys: "Each key can appear only once",
      "keys[0].weight": "Choose a weight from 1 to 10",
      "timer.intervalMs": "Choose an interval from 40 to 60,000 ms",
      stopAfter: "Choose a duration from 1 second to 24 hours",
    });
  });

  it("accepts the inclusive timer and automatic-stop boundaries", () => {
    expect(
      validateConfig(
        configWith({ timer: { intervalMs: 40 }, stopAfter: 1 }),
      ),
    ).toEqual({});
    expect(
      validateConfig(
        configWith({ timer: { intervalMs: 60_000 }, stopAfter: 86_400 }),
      ),
    ).toEqual({});
  });

  it("validates advanced natural overrides against the backend contract", () => {
    const invalid = configWith({
      mode: "natural",
      natural: {
        naturalness: 101,
        advanced: {
          minIntervalMs: 501,
          maxIntervalMs: 500,
          burstIntensity: 101,
          pauseChancePercent: 26,
        },
      },
    });

    expect(validateConfig(invalid)).toMatchObject({
      "natural.naturalness": "Choose a naturalness from 0 to 100",
      "natural.advanced": "Minimum interval cannot exceed maximum interval",
      "natural.advanced.burstIntensity":
        "Choose a burst intensity from 0 to 100",
      "natural.advanced.pauseChancePercent":
        "Choose a pause chance from 0 to 25%",
    });
  });
});
