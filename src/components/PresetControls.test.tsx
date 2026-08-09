import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_CONFIG,
  DEFAULT_PRESET,
  MAX_PRESET_NAME_LENGTH,
  validateConfig,
  type AppConfig,
} from "../domain/config";
import { PresetControls } from "./PresetControls";

function twoPresets(): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    activePresetId: "second",
    presets: [
      {
        ...DEFAULT_PRESET,
        id: "first",
        name: "First",
        keys: [{ key: "KeyA", weight: 1, cooldownMs: 0 }],
      },
      {
        ...DEFAULT_PRESET,
        id: "second",
        name: "Second",
        mode: "natural",
        stopAfter: 60,
        targetApp: { id: "com.apple.TextEdit", name: "TextEdit" },
      },
    ],
  };
}

let nextId = 0;

beforeEach(() => {
  nextId = 0;
  vi.spyOn(crypto, "randomUUID").mockImplementation(() => {
    nextId += 1;
    return `generated-${nextId}` as `${string}-${string}-${string}-${string}-${string}`;
  });
});

describe("PresetControls", () => {
  it("switches the active preset without touching any preset's contents", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );

    const select = screen.getByRole("combobox", { name: /Active preset/ });
    expect(select).toHaveValue("second");
    fireEvent.change(select, { target: { value: "first" } });

    expect(onChange).toHaveBeenCalledWith({
      ...config,
      activePresetId: "first",
    });
  });

  it("appends a new empty preset and makes it active", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New preset" }));

    expect(onChange).toHaveBeenCalledWith({
      ...config,
      activePresetId: "generated-1",
      presets: [
        ...config.presets,
        { ...DEFAULT_PRESET, id: "generated-1", name: "Preset 3" },
      ],
    });
  });

  it("duplicates the active preset's settings under a new id and name", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Duplicate preset" }));

    const next = onChange.mock.calls[0][0] as AppConfig;
    expect(next.presets).toHaveLength(3);
    expect(next.activePresetId).toBe("generated-1");
    expect(next.presets[2]).toEqual({
      ...config.presets[1],
      id: "generated-1",
      name: "Second copy",
    });
    // The source preset is untouched, and the copy is independent of it.
    expect(next.presets[1]).toEqual(config.presets[1]);
    expect(next.presets[2].targetApp).not.toBe(config.presets[1].targetApp);
    expect(validateConfig(next)).toEqual([]);
  });

  it("renames the active preset, trimming and refusing an empty name", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Rename preset" }));
    const field = screen.getByRole("textbox", { name: /Preset name/ });
    expect(field).toHaveValue("Second");

    fireEvent.change(field, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Save name" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Name the preset");

    fireEvent.change(field, { target: { value: "  Renamed  " } });
    fireEvent.click(screen.getByRole("button", { name: "Save name" }));
    expect(onChange).toHaveBeenCalledWith({
      ...config,
      presets: [config.presets[0], { ...config.presets[1], name: "Renamed" }],
    });
  });

  it("accepts a rename to another preset's exact name", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Rename preset" }));
    fireEvent.change(screen.getByRole("textbox", { name: /Preset name/ }), {
      target: { value: "First" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save name" }));

    expect(onChange).toHaveBeenCalledWith({
      ...config,
      presets: [config.presets[0], { ...config.presets[1], name: "First" }],
    });
  });

  it("refuses a name longer than sixty characters", () => {
    const onChange = vi.fn();
    render(
      <PresetControls
        config={twoPresets()}
        disabled={false}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Rename preset" }));
    const field = screen.getByRole("textbox", { name: /Preset name/ });
    fireEvent.change(field, {
      target: { value: "n".repeat(MAX_PRESET_NAME_LENGTH + 1) },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save name" }));

    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      `Keep the name to ${MAX_PRESET_NAME_LENGTH} characters`,
    );

    fireEvent.change(field, {
      target: { value: "n".repeat(MAX_PRESET_NAME_LENGTH) },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save name" }));
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("surfaces a stored name error so the save gate never blocks silently", () => {
    render(
      <PresetControls
        config={twoPresets()}
        disabled={false}
        errors={{ "presets[1].name": "Name the preset" }}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("Name the preset");

    fireEvent.click(screen.getByRole("button", { name: "Rename preset" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Name the preset");
  });

  it("deletes the active preset and activates a survivor", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete preset" }));

    expect(onChange).toHaveBeenCalledWith({
      ...config,
      activePresetId: "first",
      presets: [config.presets[0]],
    });
  });

  it("refuses to delete the last preset and explains why", () => {
    const onChange = vi.fn();
    render(
      <PresetControls
        config={DEFAULT_CONFIG}
        disabled={false}
        onChange={onChange}
      />,
    );

    const remove = screen.getByRole("button", { name: "Delete preset" });
    expect(remove).toBeDisabled();
    expect(
      screen.getByText("The last preset cannot be deleted."),
    ).toBeInTheDocument();
    expect(remove).toHaveAttribute("aria-describedby", "preset-delete-reason");

    // The handler itself refuses, not only the disabled attribute.
    fireEvent.click(remove);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("locks every preset mutation while a run holds the configuration", () => {
    const onChange = vi.fn();
    const config = twoPresets();
    const { rerender } = render(
      <PresetControls config={config} disabled={false} onChange={onChange} />,
    );
    // Open the rename form first so the locked render still has it mounted.
    fireEvent.click(screen.getByRole("button", { name: "Rename preset" }));
    fireEvent.change(screen.getByRole("textbox", { name: /Preset name/ }), {
      target: { value: "Renamed" },
    });
    rerender(
      <PresetControls config={config} disabled onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Save name" }));
    fireEvent.submit(screen.getByRole("textbox", { name: /Preset name/ }));
    expect(onChange).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    for (const name of [
      "New preset",
      "Duplicate preset",
      "Rename preset",
      "Delete preset",
    ]) {
      const button = screen.getByRole("button", { name });
      expect(button).toBeDisabled();
      fireEvent.click(button);
    }
    const select = screen.getByRole("combobox", { name: /Active preset/ });
    expect(select).toBeDisabled();
    fireEvent.change(select, { target: { value: "first" } });

    expect(onChange).not.toHaveBeenCalled();
  });
});
