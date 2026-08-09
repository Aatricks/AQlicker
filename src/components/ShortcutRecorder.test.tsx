import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ShortcutRecorder } from "./ShortcutRecorder";

const PREVIOUS = "CommandOrControl+Shift+K";

describe("ShortcutRecorder", () => {
  it("requires a modifier and rejects Escape while retaining the current shortcut", () => {
    const onRecord = vi.fn(async (candidate: string) => candidate);
    render(
      <ShortcutRecorder
        value={PREVIOUS}
        onRecord={onRecord}
        platform="Win32"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Record global shortcut" }));
    const recorder = screen.getByRole("button", { name: "Press global shortcut" });
    fireEvent.keyDown(recorder, { code: "KeyP", key: "p" });
    expect(onRecord).not.toHaveBeenCalled();
    expect(screen.getByText(/include at least one modifier/i)).toBeVisible();

    fireEvent.keyDown(recorder, { code: "Escape", key: "Escape" });
    expect(onRecord).not.toHaveBeenCalled();
    expect(screen.getByText(PREVIOUS)).toBeVisible();
    expect(screen.getByText(/Escape is reserved/i)).toBeVisible();
  });

  it("normalizes Windows Control and registers before leaving capture", async () => {
    let resolveRegistration!: (value: string) => void;
    const registration = new Promise<string>((resolve) => {
      resolveRegistration = resolve;
    });
    const onRecord = vi.fn(() => registration);
    render(
      <ShortcutRecorder
        value={PREVIOUS}
        onRecord={onRecord}
        platform="Win32"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Record global shortcut" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press global shortcut" }), {
      code: "KeyP",
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onRecord).toHaveBeenCalledWith("CommandOrControl+Shift+P");
    expect(screen.getByText(PREVIOUS)).toBeVisible();
    expect(screen.getByRole("button", { name: "Registering global shortcut" })).toBeDisabled();

    resolveRegistration("CommandOrControl+Shift+P");
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Record global shortcut" }),
      ).toBeEnabled(),
    );
  });

  it("shows an unassigned shortcut and clears one through the same control", async () => {
    const onClear = vi.fn(async () => null);
    const view = render(
      <ShortcutRecorder
        label="preset cycling shortcut"
        onClear={onClear}
        onRecord={vi.fn(async (candidate: string) => candidate)}
        platform="Win32"
        value={null}
      />,
    );

    expect(screen.getByText("Not set")).toBeVisible();
    // Nothing to clear while it is unassigned.
    expect(
      screen.queryByRole("button", { name: "Clear preset cycling shortcut" }),
    ).toBeNull();

    view.rerender(
      <ShortcutRecorder
        label="preset cycling shortcut"
        onClear={onClear}
        onRecord={vi.fn(async (candidate: string) => candidate)}
        platform="Win32"
        value="CommandOrControl+Shift+P"
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Clear preset cycling shortcut" }),
    );

    await waitFor(() => expect(onClear).toHaveBeenCalledTimes(1));
  });

  it("names its controls after the shortcut it records", () => {
    render(
      <ShortcutRecorder
        id="cycle-shortcut"
        label="preset cycling shortcut"
        onRecord={vi.fn(async (candidate: string) => candidate)}
        platform="Win32"
        title="Preset cycling shortcut"
        value={null}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "Preset cycling shortcut" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Record preset cycling shortcut" }),
    ).toBeVisible();
    // The default global-shortcut names must not leak onto this instance.
    expect(
      screen.queryByRole("button", { name: "Record global shortcut" }),
    ).toBeNull();
  });

  it("normalizes macOS Meta and retains the previous value on conflict", async () => {
    const onRecord = vi.fn(async () => {
      throw new Error("shortcut conflict");
    });
    render(
      <ShortcutRecorder
        value={PREVIOUS}
        onRecord={onRecord}
        platform="MacIntel"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Record global shortcut" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press global shortcut" }), {
      code: "KeyP",
      key: "p",
      metaKey: true,
      altKey: true,
    });

    expect(onRecord).toHaveBeenCalledWith("CommandOrControl+Alt+P");
    await waitFor(() =>
      expect(screen.getByText(/could not be registered/i)).toBeVisible(),
    );
    expect(screen.getByText(PREVIOUS)).toBeVisible();
  });
});
