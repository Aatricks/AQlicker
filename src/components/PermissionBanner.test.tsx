import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PermissionBanner } from "./PermissionBanner";

describe("PermissionBanner", () => {
  it("explains the need before any access request is made", () => {
    const onRequestAccess = vi.fn();
    render(
      <PermissionBanner
        onRequestAccess={onRequestAccess}
        requesting={false}
        status={{ granted: false, sameIntegrityOnly: false }}
      />,
    );

    expect(
      screen.getByText(
        "AQlicker needs operating-system permission to send the selected keys to the application you have in focus.",
      ),
    ).toBeVisible();
    expect(onRequestAccess).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Request access" }));
    expect(onRequestAccess).toHaveBeenCalledTimes(1);
  });

  it("reports an in-flight request without offering a second one", () => {
    const onRequestAccess = vi.fn();
    render(
      <PermissionBanner
        onRequestAccess={onRequestAccess}
        requesting
        status={{ granted: false, sameIntegrityOnly: false }}
      />,
    );

    expect(screen.getByRole("button", { name: "Requesting…" })).toBeDisabled();
  });

  it("stays out of the way once permission is granted", () => {
    const { container } = render(
      <PermissionBanner
        onRequestAccess={vi.fn()}
        requesting={false}
        status={{ granted: true, sameIntegrityOnly: false }}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("notes the Windows integrity limit without asking for access", () => {
    render(
      <PermissionBanner
        onRequestAccess={vi.fn()}
        requesting={false}
        status={{ granted: true, sameIntegrityOnly: true }}
      />,
    );

    expect(
      screen.getByText(
        "AQlicker can only send keys to applications running at the same or a lower privilege level.",
      ),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Request access" }),
    ).not.toBeInTheDocument();
  });
});
