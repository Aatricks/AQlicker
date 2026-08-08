import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { StopAfterControls } from "./StopAfterControls";

describe("StopAfterControls", () => {
  it("sets each shared quick duration, including one hour", () => {
    const onChange = vi.fn();
    render(
      <StopAfterControls value={null} onChange={onChange} disabled={false} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "5 minutes" }));
    expect(onChange).toHaveBeenLastCalledWith(300);
    fireEvent.click(screen.getByRole("button", { name: "15 minutes" }));
    expect(onChange).toHaveBeenLastCalledWith(900);
    fireEvent.click(screen.getByRole("button", { name: "30 minutes" }));
    expect(onChange).toHaveBeenLastCalledWith(1_800);
    fireEvent.click(screen.getByRole("button", { name: "1 hour" }));
    expect(onChange).toHaveBeenLastCalledWith(3_600);
  });

  it("derives and edits hours, minutes, and seconds as one total", () => {
    const onChange = vi.fn();
    render(<StopAfterControls value={3_723} onChange={onChange} />);

    expect(screen.getByRole("spinbutton", { name: "Hours" })).toHaveValue(1);
    expect(screen.getByRole("spinbutton", { name: "Minutes" })).toHaveValue(2);
    expect(screen.getByRole("spinbutton", { name: "Seconds" })).toHaveValue(3);

    fireEvent.change(screen.getByRole("spinbutton", { name: "Minutes" }), {
      target: { value: "15" },
    });
    expect(onChange).toHaveBeenLastCalledWith(4_503);
  });

  it("can disable or enable automatic stop without mode-specific state", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <StopAfterControls value={60} onChange={onChange} />,
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "Stop after" }));
    expect(onChange).toHaveBeenLastCalledWith(null);

    rerender(<StopAfterControls value={null} onChange={onChange} />);
    expect(screen.getByRole("spinbutton", { name: "Hours" })).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Stop after" }));
    expect(onChange).toHaveBeenLastCalledWith(300);
  });

  it("shows the total-duration validation error", () => {
    render(
      <StopAfterControls
        value={86_401}
        onChange={vi.fn()}
        error="Choose a duration from 1 second to 24 hours"
      />,
    );

    expect(screen.getByText(/1 second to 24 hours/)).toBeVisible();
    expect(screen.getByRole("spinbutton", { name: "Hours" })).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });
});
