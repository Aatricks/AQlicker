import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RunSnapshot } from "../api/aqlicker";
import { IDLE_SNAPSHOT } from "../hooks/useRunState";
import { RunControls } from "./RunControls";

function snapshot(overrides: Partial<RunSnapshot> = {}): RunSnapshot {
  return { ...IDLE_SNAPSHOT, ...overrides };
}

describe("RunControls", () => {
  it("lists every unmet prerequisite while Start is disabled", () => {
    render(
      <RunControls
        blockers={["Choose at least one key", "Grant input permission"]}
        onStart={vi.fn()}
        onStop={vi.fn()}
        snapshot={snapshot()}
        stopPending={false}
      />,
    );

    const start = screen.getByRole("button", { name: "Start" });
    expect(start).toBeDisabled();
    expect(screen.getByText("Choose at least one key")).toBeVisible();
    expect(screen.getByText("Grant input permission")).toBeVisible();

    // Disabled buttons leave the tab order, so the reasons must be announced
    // through the button's own description.
    const describedBy = start.getAttribute("aria-describedby");
    expect(describedBy).toBeTruthy();
    expect(document.getElementById(describedBy!)).toHaveTextContent(
      "Grant input permission",
    );
  });

  it("starts only when every prerequisite passes", () => {
    const onStart = vi.fn();
    render(
      <RunControls
        blockers={[]}
        onStart={onStart}
        onStop={vi.fn()}
        snapshot={snapshot()}
        stopPending={false}
      />,
    );

    const start = screen.getByRole("button", { name: "Start" });
    expect(start).toBeEnabled();
    fireEvent.click(start);
    expect(onStart).toHaveBeenCalledTimes(1);
  });

  it("names the active mode and shows elapsed, remaining, and press counts", () => {
    render(
      <RunControls
        blockers={[]}
        onStart={vi.fn()}
        onStop={vi.fn()}
        snapshot={snapshot({
          status: "running",
          mode: "natural",
          elapsedMs: 5_000,
          remainingMs: 3_595_000,
          successfulPresses: 21,
        })}
        stopPending={false}
      />,
    );

    expect(screen.getByText("Natural mode running")).toBeVisible();
    expect(screen.getByText("0:05 elapsed")).toBeVisible();
    expect(screen.getByText("59:55 remaining")).toBeVisible();
    expect(screen.getByText("21 presses")).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("omits remaining time when no automatic stop is configured", () => {
    render(
      <RunControls
        blockers={[]}
        onStart={vi.fn()}
        onStop={vi.fn()}
        snapshot={snapshot({
          status: "running",
          mode: "timer",
          elapsedMs: 3_661_000,
          successfulPresses: 1,
        })}
        stopPending={false}
      />,
    );

    expect(screen.getByText("Timer mode running")).toBeVisible();
    expect(screen.getByText("1:01:01 elapsed")).toBeVisible();
    expect(screen.getByText("1 press")).toBeVisible();
    expect(screen.queryByText(/remaining/)).not.toBeInTheDocument();
  });

  it("disables Stop while a stop is pending or already stopping", () => {
    const onStop = vi.fn();
    const { rerender } = render(
      <RunControls
        blockers={[]}
        onStart={vi.fn()}
        onStop={onStop}
        snapshot={snapshot({ status: "running", mode: "timer" })}
        stopPending
      />,
    );

    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).toBeDisabled();
    fireEvent.click(stop);
    expect(onStop).not.toHaveBeenCalled();

    rerender(
      <RunControls
        blockers={[]}
        onStart={vi.fn()}
        onStop={onStop}
        snapshot={snapshot({ status: "stopping", mode: "timer" })}
        stopPending={false}
      />,
    );
    expect(screen.getByText("Timer mode stopping")).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  });
});
