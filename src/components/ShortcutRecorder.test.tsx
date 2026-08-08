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

    fireEvent.click(screen.getByRole("button", { name: "Record shortcut" }));
    const recorder = screen.getByRole("button", { name: "Press shortcut" });
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

    fireEvent.click(screen.getByRole("button", { name: "Record shortcut" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press shortcut" }), {
      code: "KeyP",
      key: "p",
      ctrlKey: true,
      shiftKey: true,
    });

    expect(onRecord).toHaveBeenCalledWith("CommandOrControl+Shift+P");
    expect(screen.getByText(PREVIOUS)).toBeVisible();
    expect(screen.getByRole("button", { name: "Registering shortcut" })).toBeDisabled();

    resolveRegistration("CommandOrControl+Shift+P");
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Record shortcut" }),
      ).toBeEnabled(),
    );
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

    fireEvent.click(screen.getByRole("button", { name: "Record shortcut" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press shortcut" }), {
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
