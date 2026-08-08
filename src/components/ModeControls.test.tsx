import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_CONFIG, type AppConfig } from "../domain/config";
import { ModeControls } from "./ModeControls";

function naturalConfig(): AppConfig {
  return {
    ...DEFAULT_CONFIG,
    keys: [{ key: "KeyA", weight: 1, cooldownMs: 0 }],
    mode: "natural",
    natural: {
      naturalness: 50,
      advanced: {
        minIntervalMs: 80,
        maxIntervalMs: 450,
        burstIntensity: 40,
        pauseChancePercent: 5,
      },
    },
  };
}

describe("ModeControls", () => {
  it("shows only the 40-60,000 ms interval in Timer mode", () => {
    const onChange = vi.fn();
    render(<ModeControls config={DEFAULT_CONFIG} onChange={onChange} />);

    const interval = screen.getByRole("spinbutton", {
      name: "Timer interval (ms)",
    });
    expect(interval).toHaveAttribute("min", "40");
    expect(interval).toHaveAttribute("max", "60000");
    expect(screen.queryByRole("slider")).not.toBeInTheDocument();

    fireEvent.change(interval, { target: { value: "240" } });
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_CONFIG,
      timer: { intervalMs: 240 },
    });
  });

  it("switches mode and keeps Advanced collapsed", () => {
    const onChange = vi.fn();
    const config = naturalConfig();
    const { container } = render(
      <ModeControls config={config} onChange={onChange} />,
    );

    expect(screen.getByRole("button", { name: "Natural" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(container.querySelector("details")).not.toHaveAttribute("open");

    fireEvent.click(screen.getByRole("button", { name: "Timer" }));
    expect(onChange).toHaveBeenLastCalledWith({ ...config, mode: "timer" });
  });

  it("clears advanced overrides whenever the Naturalness slider moves", () => {
    const onChange = vi.fn();
    const config = naturalConfig();
    render(<ModeControls config={config} onChange={onChange} />);

    const slider = screen.getByRole("slider", { name: "Naturalness" });
    expect(slider).toHaveValue("50");
    fireEvent.change(slider, { target: { value: "65" } });

    expect(onChange).toHaveBeenLastCalledWith({
      ...config,
      natural: { naturalness: 65, advanced: null },
    });
  });

  it("creates a complete advanced override set when a field changes", () => {
    const onChange = vi.fn();
    const config: AppConfig = {
      ...naturalConfig(),
      natural: { naturalness: 50, advanced: null },
    };
    render(<ModeControls config={config} onChange={onChange} />);
    fireEvent.click(screen.getByText("Advanced"));
    fireEvent.change(
      screen.getByRole("spinbutton", { name: "Minimum interval (ms)" }),
      { target: { value: "120" } },
    );

    expect(onChange).toHaveBeenLastCalledWith({
      ...config,
      natural: {
        naturalness: 50,
        advanced: {
          minIntervalMs: 120,
          maxIntervalMs: 350,
          burstIntensity: 54,
          pauseChancePercent: 7,
        },
      },
    });
  });

  it.each([
    [0, 140, 220, 8, 1],
    [50, 98, 350, 54, 7],
    [100, 55, 480, 100, 12],
  ])(
    "derives Advanced defaults from Naturalness %i using scheduler interpolation",
    (naturalness, minimum, maximum, burst, pause) => {
      const config: AppConfig = {
        ...naturalConfig(),
        natural: { naturalness, advanced: null },
      };
      render(<ModeControls config={config} onChange={vi.fn()} />);
      fireEvent.click(screen.getByText("Advanced"));

      expect(
        screen.getByRole("spinbutton", { name: "Minimum interval (ms)" }),
      ).toHaveValue(minimum);
      expect(
        screen.getByRole("spinbutton", { name: "Maximum interval (ms)" }),
      ).toHaveValue(maximum);
      expect(
        screen.getByRole("spinbutton", { name: "Burst intensity" }),
      ).toHaveValue(burst);
      expect(
        screen.getByRole("spinbutton", { name: "Pause chance (%)" }),
      ).toHaveValue(pause);
    },
  );

  it("associates a mode field error with the invalid input", () => {
    render(
      <ModeControls
        config={DEFAULT_CONFIG}
        onChange={vi.fn()}
        errors={{
          "timer.intervalMs": "Choose an interval from 40 to 60,000 ms",
        }}
      />,
    );

    expect(
      screen.getByRole("spinbutton", { name: "Timer interval (ms)" }),
    ).toHaveAccessibleDescription(
      "Choose an interval from 40 to 60,000 ms",
    );
  });
});
