import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  DEFAULT_PRESET,
  MAX_PRESET_NAME_LENGTH,
  type AppConfig,
  type Preset,
} from "./config";
import { validateConfig, validateConfigForStart } from "./validation";

/**
 * Only the active preset is on screen, so its messages stay unprefixed. The
 * inactive preset at index 0 is here to prove nothing leaks in from it.
 */
function configWith(overrides: Partial<Preset>): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    activePresetId: "second",
    presets: [
      { ...DEFAULT_PRESET, id: "first", name: "First" },
      {
        ...DEFAULT_PRESET,
        id: "second",
        name: "Second",
        keys: [{ key: "KeyA", weight: 1, cooldownMs: 0 }],
        ...overrides,
      },
    ],
  };
}

describe("configuration draft validation", () => {
  it("reports duplicate, duration, and mode-specific field errors", () => {
    const invalid = configWith({
      keys: [
        { key: "KeyA", weight: 0, cooldownMs: 60_001 },
        { key: "KeyA", weight: 1, cooldownMs: 0 },
      ],
      timer: { intervalMs: 60_001 },
      stopAfter: 86_401,
    });

    expect(validateConfig(invalid)).toMatchObject({
      keys: "Each key can appear only once",
      "keys[0].weight": "Choose a weight from 1 to 10",
      "keys[0].cooldownMs": "Choose a cooldown from 0 to 60,000 ms",
      "timer.intervalMs": "Choose an interval from 40 to 60,000 ms",
      stopAfter: "Choose a duration from 1 second to 24 hours",
    });
  });

  it("allows empty keys for persistence but requires one to start", () => {
    expect(validateConfig(DEFAULT_CONFIG)).toEqual({});
    expect(validateConfigForStart(DEFAULT_CONFIG)).toMatchObject({
      keys: "Choose at least one key",
    });
    expect(validateConfig(configWith({ keys: [] }))).toEqual({});
    expect(validateConfigForStart(configWith({ keys: [] }))).toMatchObject({
      keys: "Choose at least one key",
    });
  });

  it("reports a bad name for every preset, not only the active one", () => {
    const inactiveIsUnnamed: AppConfig = {
      ...configWith({}),
      presets: configWith({}).presets.map((preset, index) =>
        index === 0 ? { ...preset, name: "  " } : preset,
      ),
    };

    // Otherwise the save gate blocks on an error no control is showing.
    expect(validateConfig(inactiveIsUnnamed)).toEqual({
      "presets[0].name": "Name the preset",
    });
    expect(validateConfig(configWith({ name: "  " }))).toEqual({
      name: "Name the preset",
      "presets[1].name": "Name the preset",
    });
    expect(
      validateConfig(configWith({ name: "n".repeat(MAX_PRESET_NAME_LENGTH + 1) })),
    ).toMatchObject({ name: "Keep the name to 60 characters" });
  });

  it("reports an unresolvable active preset instead of validating nothing", () => {
    expect(
      validateConfig({ ...configWith({}), activePresetId: "missing" }),
    ).toEqual({ activePresetId: "Choose a preset" });
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
