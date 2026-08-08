import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RunningApp } from "../api/aqlicker";
import { TargetAppPicker } from "./TargetAppPicker";

const APPS: RunningApp[] = [
  { id: "com.apple.Safari", name: "Safari" },
  { id: "com.apple.TextEdit", name: "TextEdit" },
];

describe("TargetAppPicker", () => {
  it("stores the stable identifier and shows the friendly name", async () => {
    const onChange = vi.fn();
    render(
      <TargetAppPicker
        disabled={false}
        listApps={() => Promise.resolve(APPS)}
        onChange={onChange}
        value={null}
      />,
    );

    const select = await screen.findByLabelText("Restrict to application");
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "TextEdit" })).toBeInTheDocument(),
    );
    fireEvent.change(select, { target: { value: "com.apple.TextEdit" } });

    expect(onChange).toHaveBeenCalledWith({
      id: "com.apple.TextEdit",
      name: "TextEdit",
    });
  });

  it("turns the restriction off and keeps a stored application that is not running", async () => {
    const onChange = vi.fn();
    render(
      <TargetAppPicker
        disabled={false}
        listApps={() => Promise.resolve(APPS)}
        onChange={onChange}
        value={{ id: "com.apple.Terminal", name: "Terminal" }}
      />,
    );

    const select = await screen.findByLabelText("Restrict to application");
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Terminal" })).toBeInTheDocument(),
    );
    expect(select).toHaveValue("com.apple.Terminal");

    fireEvent.change(select, { target: { value: "" } });
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("stays usable when the application list cannot be read", async () => {
    render(
      <TargetAppPicker
        disabled={false}
        listApps={() => Promise.reject(new Error("nope"))}
        onChange={vi.fn()}
        value={null}
      />,
    );

    const select = await screen.findByLabelText("Restrict to application");
    await waitFor(() => expect(select).toBeEnabled());
    expect(screen.getByRole("option", { name: "Any application" })).toBeInTheDocument();
  });
});
