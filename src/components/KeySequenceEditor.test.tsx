import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AppConfig } from "../domain/config";
import { KeySequenceEditor } from "./KeySequenceEditor";

type KeyEntry = AppConfig["keys"][number];

const entry = (
  key: KeyEntry["key"],
  weight = 1,
  cooldownMs = 0,
): KeyEntry => ({ key, weight, cooldownMs });

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
    const capture = screen.getByRole("button", {
      name: "Physical key capture",
    });
    fireEvent.click(capture);
    fireEvent.keyDown(capture, { code: "Numpad1", key: "1" });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByText(/not supported/i)).toBeVisible();

    fireEvent.keyDown(capture, { code: "KeyA", key: "a" });
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
    expect(screen.getByRole("button", { name: "Add key" })).toHaveFocus();
  });

  it("does not capture keyboard navigation or control activation bubbling in the dialog", () => {
    const onChange = vi.fn();
    render(
      <KeySequenceEditor
        value={[entry("KeyA")]}
        onChange={onChange}
        mode="timer"
      />,
    );

    const addKey = screen.getByRole("button", { name: "Add key" });
    fireEvent.click(addKey);
    const capture = screen.getByRole("button", {
      name: "Physical key capture",
    });
    expect(capture).toHaveFocus();
    fireEvent.keyDown(capture, { code: "Tab", key: "Tab" });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeVisible();

    const search = screen.getByRole("searchbox", { name: "Search keys" });
    search.focus();
    fireEvent.keyDown(search, { code: "Tab", key: "Tab" });
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeVisible();

    const close = screen.getByRole("button", { name: "Close key picker" });
    fireEvent.keyDown(close, { code: "Space", key: " " });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(close);
    expect(addKey).toHaveFocus();

    fireEvent.click(addKey);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search keys" }), {
      target: { value: "KeyB" },
    });
    const keyOption = screen.getByRole("button", { name: /B.*KeyB/ });
    fireEvent.keyDown(keyOption, { code: "Enter", key: "Enter" });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(keyOption);
    expect(onChange).toHaveBeenCalledWith([entry("KeyA"), entry("KeyB")]);
    expect(addKey).toHaveFocus();
  });

  it("contains dialog focus and restores Add key on Escape", () => {
    render(
      <KeySequenceEditor
        value={[entry("KeyA")]}
        onChange={vi.fn()}
        mode="timer"
      />,
    );

    const addKey = screen.getByRole("button", { name: "Add key" });
    fireEvent.click(addKey);
    const dialog = screen.getByRole("dialog");
    const dialogButtons = within(dialog).getAllByRole("button");
    const first = dialogButtons[0];
    const last = dialogButtons.at(-1)!;

    first.focus();
    fireEvent.keyDown(first, { code: "Tab", key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();

    fireEvent.keyDown(last, { code: "Tab", key: "Tab" });
    expect(first).toHaveFocus();

    const search = screen.getByRole("searchbox", { name: "Search keys" });
    search.focus();
    fireEvent.keyDown(search, { code: "Escape", key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(addKey).toHaveFocus();
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

  it("exposes a 0-60,000 ms cooldown only in Natural mode", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <KeySequenceEditor
        value={[entry("Space", 3, 250)]}
        onChange={onChange}
        mode="natural"
      />,
    );

    const cooldown = screen.getByRole("spinbutton", {
      name: "Space cooldown in milliseconds",
    });
    expect(cooldown).toHaveAttribute("min", "0");
    expect(cooldown).toHaveAttribute("max", "60000");
    fireEvent.change(cooldown, { target: { value: "1500" } });
    expect(onChange).toHaveBeenLastCalledWith([entry("Space", 3, 1_500)]);

    rerender(
      <KeySequenceEditor
        value={[entry("Space", 3, 250)]}
        onChange={onChange}
        mode="timer"
      />,
    );
    expect(
      screen.queryByRole("spinbutton", {
        name: "Space cooldown in milliseconds",
      }),
    ).toBeNull();
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

  it("closes the picker and refuses selection when a run locks the editor", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <KeySequenceEditor
        value={[entry("KeyA")]}
        onChange={onChange}
        mode="timer"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add key" }));
    const capture = screen.getByRole("button", { name: "Physical key capture" });
    fireEvent.click(capture);

    // A global toggle can start a run while the dialog is open; the dialog can
    // never intercept that OS-level shortcut.
    rerender(
      <KeySequenceEditor
        value={[entry("KeyA")]}
        onChange={onChange}
        mode="timer"
        disabled
      />,
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.keyDown(capture, { code: "Space", key: " " });
    expect(onChange).not.toHaveBeenCalled();
  });
});
