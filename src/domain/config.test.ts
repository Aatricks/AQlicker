import fixtureV1 from "../../src-tauri/tests/fixtures/config-v1.json";
import fixtureV2 from "../../src-tauri/tests/fixtures/config-v2.json";
import fixtureV3 from "../../src-tauri/tests/fixtures/config-v3.json";
import fixture from "../../src-tauri/tests/fixtures/config-v4.json";
import logicalKeys from "../../src-tauri/tests/fixtures/logical-keys.json";
import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  DEFAULT_PRESET,
  LOGICAL_KEYS,
  MAX_PRESET_NAME_LENGTH,
  activePreset,
  serializeConfig,
  validateConfig,
  validateConfigForStart,
  type AppConfig,
  type Preset,
} from "./config";

/**
 * The preset under test is index 1 throughout, so a hard-coded `presets[0].`
 * prefix in the production code cannot satisfy these assertions.
 */
function configWith(overrides: Partial<Preset>): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    activePresetId: "second",
    presets: [
      { ...DEFAULT_PRESET, id: "first", name: "First" },
      { ...DEFAULT_PRESET, id: "second", name: "Second", ...overrides },
    ],
  };
}

describe("configuration contract", () => {
  it("matches the exhaustive supported-key golden list without duplicates", () => {
    expect(new Set(logicalKeys).size).toBe(logicalKeys.length);
    expect(new Set(LOGICAL_KEYS).size).toBe(LOGICAL_KEYS.length);
    expect(LOGICAL_KEYS).toEqual(logicalKeys);
  });

  it("keeps the v1 fixture readable as the pre-target-application schema", () => {
    expect(fixtureV1.schemaVersion).toBe(1);
    expect(fixtureV1).not.toHaveProperty("targetApp");
  });

  it("keeps the v2 fixture readable as the pre-cooldown schema", () => {
    expect(fixtureV2.schemaVersion).toBe(2);
    expect(fixtureV2.keys.every((entry) => !("cooldownMs" in entry))).toBe(true);
  });

  it("keeps the v3 fixture readable as the pre-preset schema", () => {
    expect(fixtureV3.schemaVersion).toBe(3);
    expect(fixtureV3).not.toHaveProperty("presets");
    expect(fixtureV3).toHaveProperty("globalShortcut");
  });

  it("deserializes the v4 fixture with its exact camelCase schema", () => {
    const config = fixture as AppConfig;

    expect(config).toEqual({
      schemaVersion: 4,
      globalShortcut: "CommandOrControl+Shift+K",
      activePresetId: "preset-grinding",
      presets: [
        {
          id: "default",
          name: "Default",
          keys: [
            { key: "KeyA", weight: 3, cooldownMs: 0 },
            { key: "Digit1", weight: 2, cooldownMs: 250 },
          ],
          mode: "timer",
          timer: { intervalMs: 100 },
          natural: { naturalness: 50, advanced: null },
          stopAfter: null,
          targetApp: null,
        },
        {
          id: "preset-grinding",
          name: "Grinding",
          keys: [
            { key: "F12", weight: 1, cooldownMs: 60_000 },
            { key: "ArrowUp", weight: 4, cooldownMs: 0 },
            { key: "Space", weight: 5, cooldownMs: 1_500 },
            { key: "Backquote", weight: 1, cooldownMs: 0 },
          ],
          mode: "natural",
          timer: { intervalMs: 120 },
          natural: {
            naturalness: 65,
            advanced: {
              minIntervalMs: 80,
              maxIntervalMs: 400,
              burstIntensity: 35,
              pauseChancePercent: 10,
            },
          },
          stopAfter: 3600,
          targetApp: { id: "com.apple.TextEdit", name: "TextEdit" },
        },
      ],
    });
    expect(activePreset(config)?.name).toBe("Grinding");
    expect(validateConfig(config)).toEqual([]);
    expect(serializeConfig(config)).toBe(JSON.stringify(fixture));
  });

  it("reports field-specific validation errors and start-only empty selection", () => {
    const config = configWith({
      keys: [
        { key: "KeyA", weight: 0, cooldownMs: 60_001 },
        { key: "KeyA", weight: 1, cooldownMs: 0 },
      ],
      timer: { intervalMs: 60_001 },
      natural: {
        naturalness: 50,
        advanced: {
          minIntervalMs: 5_001,
          maxIntervalMs: 39,
          burstIntensity: 101,
          pauseChancePercent: -1,
        },
      },
      stopAfter: 86_401,
    });

    expect(validateConfig(config)).toEqual(expect.arrayContaining([
      { field: "presets[1].keys", code: "duplicate" },
      { field: "presets[1].keys[0].weight", code: "range" },
      { field: "presets[1].keys[0].cooldownMs", code: "range" },
      { field: "presets[1].timer.intervalMs", code: "range" },
      { field: "presets[1].natural.advanced.minIntervalMs", code: "range" },
      { field: "presets[1].natural.advanced.maxIntervalMs", code: "range" },
      { field: "presets[1].natural.advanced", code: "ordering" },
      { field: "presets[1].natural.advanced.burstIntensity", code: "range" },
      { field: "presets[1].natural.advanced.pauseChancePercent", code: "range" },
      { field: "presets[1].stopAfter", code: "range" },
    ]));
    expect(
      validateConfig(config).filter(({ field }) => field.startsWith("presets[0]")),
    ).toEqual([]);
    expect(validateConfigForStart(configWith({}))).toContainEqual({
      field: "presets[1].keys",
      code: "required",
    });
    expect(validateConfigForStart(DEFAULT_CONFIG)).toContainEqual({
      field: "presets[0].keys",
      code: "required",
    });
  });

  it("bounds the per-key cooldown from 0 to 60,000 ms", () => {
    const configWithCooldown = (cooldownMs: number) =>
      configWith({ keys: [{ key: "KeyA", weight: 1, cooldownMs }] });

    expect(validateConfig(configWithCooldown(0))).toEqual([]);
    expect(validateConfig(configWithCooldown(60_000))).toEqual([]);
    for (const cooldownMs of [-1, 0.5, 60_001]) {
      expect(validateConfig(configWithCooldown(cooldownMs))).toContainEqual({
        field: "presets[1].keys[0].cooldownMs",
        code: "range",
      });
    }
  });

  it("rejects a target application without a stable identifier", () => {
    expect(validateConfig(configWith({ targetApp: { id: " ", name: "Ghost" } })))
      .toContainEqual({ field: "presets[1].targetApp.id", code: "required" });
    expect(validateConfig(configWith({ targetApp: { id: "com.apple.TextEdit", name: "TextEdit" } })))
      .toEqual([]);
  });

  it("caps advanced pause chance at twenty-five percent", () => {
    const configWithPauseChance = (pauseChancePercent: number) =>
      configWith({
        natural: {
          naturalness: 50,
          advanced: {
            minIntervalMs: 100,
            maxIntervalMs: 500,
            burstIntensity: 50,
            pauseChancePercent,
          },
        },
      });

    expect(validateConfig(configWithPauseChance(25))).toEqual([]);
    for (const pauseChancePercent of [26, 100]) {
      expect(validateConfig(configWithPauseChance(pauseChancePercent))).toContainEqual({
        field: "presets[1].natural.advanced.pauseChancePercent",
        code: "range",
      });
    }
  });

  it("trims preset names and bounds them at sixty characters", () => {
    expect(validateConfig(configWith({ name: "A" }))).toEqual([]);
    expect(validateConfig(configWith({ name: "n".repeat(MAX_PRESET_NAME_LENGTH) }))).toEqual([]);
    expect(
      validateConfig(configWith({ name: `${"n".repeat(MAX_PRESET_NAME_LENGTH)}   ` })),
    ).toEqual([]);
    for (const name of ["", "   ", "\t"]) {
      expect(validateConfig(configWith({ name }))).toContainEqual({
        field: "presets[1].name",
        code: "required",
      });
    }
    expect(
      validateConfig(configWith({ name: "n".repeat(MAX_PRESET_NAME_LENGTH + 1) })),
    ).toContainEqual({ field: "presets[1].name", code: "range" });
  });

  it("allows two presets to share a name", () => {
    // Identity is the id. Two presets may be named the same on purpose.
    const shared: AppConfig = {
      ...configWith({ name: "Grinding" }),
      presets: [
        { ...DEFAULT_PRESET, id: "first", name: "Grinding" },
        { ...DEFAULT_PRESET, id: "second", name: "Grinding" },
      ],
    };

    expect(validateConfig(shared)).toEqual([]);
  });

  it("requires a resolvable active preset and at least one preset", () => {
    expect(
      validateConfig({ ...configWith({}), activePresetId: "missing" }),
    ).toContainEqual({ field: "activePresetId", code: "unknown" });
    expect(activePreset({ ...configWith({}), activePresetId: "missing" })).toBeNull();
    expect(
      validateConfig({ ...DEFAULT_CONFIG, presets: [] }),
    ).toContainEqual({ field: "presets", code: "required" });
  });

  it("rejects duplicate and empty preset identifiers", () => {
    const duplicated: AppConfig = {
      ...DEFAULT_CONFIG,
      activePresetId: "same",
      presets: [
        { ...DEFAULT_PRESET, id: "same", name: "One" },
        { ...DEFAULT_PRESET, id: "same", name: "Two" },
      ],
    };
    expect(validateConfig(duplicated)).toContainEqual({
      field: "presets",
      code: "duplicate",
    });
    expect(validateConfig(configWith({ id: "  " }))).toContainEqual({
      field: "presets[1].id",
      code: "required",
    });
  });

  it("starts from exactly one preset named Default", () => {
    expect(DEFAULT_CONFIG.presets).toEqual([DEFAULT_PRESET]);
    expect(DEFAULT_PRESET.id).toBe("default");
    expect(DEFAULT_PRESET.name).toBe("Default");
    expect(validateConfig(DEFAULT_CONFIG)).toEqual([]);
  });
});
