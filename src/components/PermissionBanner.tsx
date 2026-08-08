import type { PermissionStatus } from "../api/aqlicker";

interface PermissionBannerProps {
  status: PermissionStatus;
  requesting: boolean;
  onRequestAccess: () => void;
}

export function PermissionBanner({
  status,
  requesting,
  onRequestAccess,
}: PermissionBannerProps) {
  if (!status.granted) {
    return (
      <section className="permission-banner" aria-labelledby="permission-title">
        <h2 id="permission-title">Input permission needed</h2>
        <p>
          AQlicker needs operating-system permission to send the selected keys to
          the application you have in focus.
        </p>
        <button disabled={requesting} onClick={onRequestAccess} type="button">
          {requesting ? "Requesting…" : "Request access"}
        </button>
      </section>
    );
  }

  if (status.sameIntegrityOnly) {
    return (
      <section className="permission-banner permission-note">
        <p>
          AQlicker can only send keys to applications running at the same or a
          lower privilege level.
        </p>
      </section>
    );
  }

  return null;
}
