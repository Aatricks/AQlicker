import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AqlickerApi, BootstrapPayload } from "./api/aqlicker";
import { DEFAULT_CONFIG } from "./domain/config";
import App from "./App";

function fakeApi(keys: BootstrapPayload["config"]["keys"] = []) {
  const payload: BootstrapPayload = {
    config: { ...DEFAULT_CONFIG, keys },
    recoveryNotice: null,
    permission: { granted: true, sameIntegrityOnly: false },
    shortcut: {
      shortcut: DEFAULT_CONFIG.globalShortcut,
      registered: true,
      error: null,
    },
    run: {
      status: "idle",
      mode: null,
      elapsedMs: 0,
      remainingMs: null,
      successfulPresses: 0,
      stopReason: null,
      error: null,
    },
  };
  return {
    bootstrap: vi.fn(async () => payload),
    saveConfig: vi.fn(async () => undefined),
    setShortcut: vi.fn(async (shortcut: string) => shortcut),
  } as unknown as AqlickerApi;
}

describe("App", () => {
  it("renders the idle AQlicker shell and required-key validation", async () => {
    render(<App api={fakeApi()} />);

    expect(screen.getByRole("heading", { name: "AQlicker" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
    expect(await screen.findByText("Choose at least one key")).toBeVisible();
    expect(screen.getByRole("button", { name: "Add key" })).toBeEnabled();
  });

  it("composes mode weights and the shared automatic-stop draft", async () => {
    render(<App api={fakeApi([{ key: "Space", weight: 3 }])} />);
    await screen.findByRole("button", { name: "Add key" });

    fireEvent.click(screen.getByRole("button", { name: "Natural" }));
    expect(
      screen.getByRole("spinbutton", { name: "Space frequency weight" }),
    ).toHaveValue(3);

    fireEvent.click(screen.getByRole("button", { name: "1 hour" }));
    expect(screen.getByRole("spinbutton", { name: "Hours" })).toHaveValue(1);
    expect(
      screen.getByRole("heading", { name: "Global shortcut" }),
    ).toBeVisible();
  });
});
