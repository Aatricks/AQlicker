import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  it("renders the idle AQlicker shell", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "AQlicker" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
  });
});
