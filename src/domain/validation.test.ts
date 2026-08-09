import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  DEFAULT_PRESET,
  MAX_PRESET_NAME_LENGTH,
  type AppConfig,
  type Preset,
} from "./config";
import { validateConfig as strictValidate } from "./config";
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

    // Otherwise the save gate blocks on an error no control is showing. An
    // unnamed preset falls back to its position for the label.
    expect(validateConfig(inactiveIsUnnamed)).toEqual({
      "presets[0].name": "Preset 1: Name the preset",
    });
    expect(validateConfig(configWith({ name: "  " }))).toEqual({
      name: "Name the preset",
    });
    expect(
      validateConfig(configWith({ name: "n".repeat(MAX_PRESET_NAME_LENGTH + 1) })),
    ).toMatchObject({ name: "Keep the name to 60 characters" });
  });

  it("blocks on an invalid field in a preset that is not active", () => {
    // The UI gate must cover exactly what Rust rejects, or the save fires,
    // the backend refuses it, and every later edit is lost.
    const config: AppConfig = {
      ...configWith({}),
      presets: [
        {
          ...DEFAULT_PRESET,
          id: "first",
          name: "First",
          timer: { intervalMs: 39 },
        },
        { ...DEFAULT_PRESET, id: "second", name: "Second" },
      ],
    };

    expect(validateConfig(config)).toEqual({
      "presets[0].timer.intervalMs":
        'Preset "First": Choose an interval from 40 to 60,000 ms',
    });
    expect(validateConfigForStart(config)).toMatchObject({
      "presets[0].timer.intervalMs":
        'Preset "First": Choose an interval from 40 to 60,000 ms',
      keys: "Choose at least one key",
    });
  });

  it("covers every rejection the Rust validator makes, on every preset", () => {
    const broken: Preset = {
      ...DEFAULT_PRESET,
      keys: [
        { key: "KeyA", weight: 0, cooldownMs: 60_001 },
        { key: "KeyA", weight: 1, cooldownMs: 0 },
      ],
      timer: { intervalMs: 39 },
      natural: {
        naturalness: 101,
        advanced: {
          minIntervalMs: 5_001,
          maxIntervalMs: 39,
          burstIntensity: 101,
          pauseChancePercent: 26,
        },
      },
      stopAfter: 86_401,
      targetApp: { id: " ", name: "Ghost" },
      name: "  ",
    };

    const cases: AppConfig[] = [
      // Every field wrong, on a preset that is not the active one.
      {
        ...DEFAULT_CONFIG,
        activePresetId: "second",
        presets: [
          { ...broken, id: "first" },
          { ...DEFAULT_PRESET, id: "second", name: "Second" },
        ],
      },
      // The same, on the active one.
      {
        ...DEFAULT_CONFIG,
        activePresetId: "first",
        presets: [
          { ...broken, id: "first" },
          { ...DEFAULT_PRESET, id: "second", name: "Second" },
        ],
      },
      // Structural rejections: blank id, duplicate id, unresolvable active,
      // and an empty list.
      {
        ...DEFAULT_CONFIG,
        activePresetId: "missing",
        presets: [{ ...DEFAULT_PRESET, id: "  ", name: "Blank" }],
      },
      {
        ...DEFAULT_CONFIG,
        activePresetId: "same",
        presets: [
          { ...DEFAULT_PRESET, id: "same", name: "One" },
          { ...DEFAULT_PRESET, id: "same", name: "Two" },
        ],
      },
      { ...DEFAULT_CONFIG, presets: [] },
    ];

    for (const config of cases) {
      // The active preset's messages are unprefixed, because its controls are
      // the ones on screen. Normalise the strict paths the same way.
      const activeIndex = config.presets.findIndex(
        (preset) => preset.id === config.activePresetId,
      );
      const prefix = `presets[${activeIndex}].`;
      const strict = strictValidate(config).map(({ field }) =>
        activeIndex >= 0 && field.startsWith(prefix)
          ? field.slice(prefix.length)
          : field,
      );
      expect(strict.length).toBeGreaterThan(0);
      // A field Rust rejects but this gate accepts means a save that fires,
      // gets refused, and strands every later edit in React state.
      expect(new Set(Object.keys(validateConfig(config)))).toEqual(
        new Set(strict),
      );
    }
  });

  it("reports an unresolvable active preset instead of validating nothing", () => {
    expect(
      validateConfig({ ...configWith({}), activePresetId: "missing" }),
    ).toEqual({ activePresetId: "Choose a preset" });
    expect(validateConfig({ ...DEFAULT_CONFIG, presets: [] })).toMatchObject({
      presets: "Keep at least one preset",
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
