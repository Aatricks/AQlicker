import fixtureV1 from "../../src-tauri/tests/fixtures/config-v1.json";
import fixtureV2 from "../../src-tauri/tests/fixtures/config-v2.json";
import fixture from "../../src-tauri/tests/fixtures/config-v3.json";
import logicalKeys from "../../src-tauri/tests/fixtures/logical-keys.json";
import { describe, expect, it } from "vitest";
import {
  DEFAULT_CONFIG,
  LOGICAL_KEYS,
  serializeConfig,
  validateConfig,
  validateConfigForStart,
  type AppConfig,
} from "./config";

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

  it("deserializes the v3 fixture with its exact camelCase schema", () => {
    const config = fixture as AppConfig;

    expect(config).toEqual({
      schemaVersion: 3,
      keys: [
        { key: "KeyA", weight: 3, cooldownMs: 0 },
        { key: "Digit1", weight: 2, cooldownMs: 250 },
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
      globalShortcut: "CommandOrControl+Shift+K",
      targetApp: { id: "com.apple.TextEdit", name: "TextEdit" },
    });
    expect(serializeConfig(config)).toBe(JSON.stringify(fixture));
  });

  it("reports field-specific validation errors and start-only empty selection", () => {
    const config: AppConfig = {
      ...DEFAULT_CONFIG,
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
    };

    expect(validateConfig(config)).toEqual(expect.arrayContaining([
      { field: "keys", code: "duplicate" },
      { field: "keys[0].weight", code: "range" },
      { field: "keys[0].cooldownMs", code: "range" },
      { field: "timer.intervalMs", code: "range" },
      { field: "natural.advanced.minIntervalMs", code: "range" },
      { field: "natural.advanced.maxIntervalMs", code: "range" },
      { field: "natural.advanced", code: "ordering" },
      { field: "stopAfter", code: "range" },
    ]));
    expect(validateConfigForStart(DEFAULT_CONFIG)).toContainEqual({ field: "keys", code: "required" });
  });

  it("bounds the per-key cooldown from 0 to 60,000 ms", () => {
    const configWithCooldown = (cooldownMs: number): AppConfig => ({
      ...DEFAULT_CONFIG,
      keys: [{ key: "KeyA", weight: 1, cooldownMs }],
    });

    expect(validateConfig(configWithCooldown(0))).toEqual([]);
    expect(validateConfig(configWithCooldown(60_000))).toEqual([]);
    for (const cooldownMs of [-1, 0.5, 60_001]) {
      expect(validateConfig(configWithCooldown(cooldownMs))).toContainEqual({
        field: "keys[0].cooldownMs",
        code: "range",
      });
    }
  });

  it("rejects a target application without a stable identifier", () => {
    expect(validateConfig({ ...DEFAULT_CONFIG, targetApp: { id: " ", name: "Ghost" } }))
      .toContainEqual({ field: "targetApp.id", code: "required" });
    expect(validateConfig({ ...DEFAULT_CONFIG, targetApp: { id: "com.apple.TextEdit", name: "TextEdit" } }))
      .toEqual([]);
  });

  it("caps advanced pause chance at twenty-five percent", () => {
    const configWithPauseChance = (pauseChancePercent: number): AppConfig => ({
      ...DEFAULT_CONFIG,
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
        field: "natural.advanced.pauseChancePercent",
        code: "range",
      });
    }
  });
});
