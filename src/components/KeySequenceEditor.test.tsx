import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AppConfig } from "../domain/config";
import { KeySequenceEditor } from "./KeySequenceEditor";

type KeyEntry = AppConfig["keys"][number];

const entry = (key: KeyEntry["key"], weight = 1): KeyEntry => ({ key, weight });

describe("KeySequenceEditor", () => {
  it("captures a supported physical code and focuses rather than duplicates it", () => {
    const onChange = vi.fn();
    render(
      <KeySequenceEditor
        value={[entry("KeyA")]}
        onChange={onChange}
        mode="timer"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add key" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.keyDown(dialog, { code: "Numpad1", key: "1" });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText(/not supported/i)).toBeVisible();

    fireEvent.keyDown(dialog, { code: "KeyA", key: "a" });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("key-KeyA")).toHaveFocus();
  });

  it("searches the supported catalogue and adds the selected physical key", () => {
    const onChange = vi.fn();
    render(
      <KeySequenceEditor
        value={[entry("KeyA")]}
        onChange={onChange}
        mode="timer"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add key" }));
    fireEvent.change(screen.getByRole("searchbox", { name: "Search keys" }), {
      target: { value: "space" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Space.*Space/ }));

    expect(onChange).toHaveBeenCalledWith([
      entry("KeyA"),
      entry("Space"),
    ]);
  });

  it("removes and reorders keys with buttons and pointer drag", () => {
    const onChange = vi.fn();
    const value = [entry("KeyA"), entry("KeyB"), entry("KeyC")];
    render(
      <KeySequenceEditor value={value} onChange={onChange} mode="timer" />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Move B left" }));
    expect(onChange).toHaveBeenLastCalledWith([
      entry("KeyB"),
      entry("KeyA"),
      entry("KeyC"),
    ]);

    fireEvent.dragStart(screen.getByTestId("key-KeyA"));
    fireEvent.dragOver(screen.getByTestId("key-KeyC"));
    fireEvent.drop(screen.getByTestId("key-KeyC"));
    expect(onChange).toHaveBeenLastCalledWith([
      entry("KeyB"),
      entry("KeyC"),
      entry("KeyA"),
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Remove B" }));
    expect(onChange).toHaveBeenLastCalledWith([
      entry("KeyA"),
      entry("KeyC"),
    ]);
  });

  it("exposes 1-10 frequency weights only in Natural mode", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <KeySequenceEditor
        value={[entry("Space", 3)]}
        onChange={onChange}
        mode="natural"
      />,
    );

    const weight = screen.getByRole("spinbutton", {
      name: "Space frequency weight",
    });
    expect(weight).toHaveAttribute("min", "1");
    expect(weight).toHaveAttribute("max", "10");
    fireEvent.change(weight, { target: { value: "7" } });
    expect(onChange).toHaveBeenLastCalledWith([entry("Space", 7)]);

    rerender(
      <KeySequenceEditor
        value={[entry("Space", 3)]}
        onChange={onChange}
        mode="timer"
      />,
    );
    expect(
      within(screen.getByRole("list", { name: "Selected keys" })).queryByRole(
        "spinbutton",
      ),
    ).not.toBeInTheDocument();
  });

  it("associates a weight validation error with its Natural input", () => {
    render(
      <KeySequenceEditor
        value={[entry("Space", 11)]}
        onChange={vi.fn()}
        mode="natural"
        errors={{ "keys[0].weight": "Choose a weight from 1 to 10" }}
      />,
    );

    expect(
      screen.getByRole("spinbutton", { name: "Space frequency weight" }),
    ).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Choose a weight from 1 to 10")).toBeVisible();
  });
});
