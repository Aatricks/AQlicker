import { useEffect, useState } from "react";
import type { RunningApp } from "../api/aqlicker";
import type { TargetApp } from "../domain/config";

interface TargetAppPickerProps {
  value: TargetApp | null;
  disabled: boolean;
  listApps: () => Promise<RunningApp[]>;
  onChange: (target: TargetApp | null) => void;
}

/**
 * Off by default. The stable platform identifier is what gets stored; the
 * friendly name is only for display, and it is kept alongside the identifier so
 * a stored target that is not running right now still shows its own name.
 */
export function TargetAppPicker({
  value,
  disabled,
  listApps,
  onChange,
}: TargetAppPickerProps) {
  const [apps, setApps] = useState<RunningApp[]>([]);

  useEffect(() => {
    let active = true;
    void listApps()
      .then((running) => {
        if (active) setApps(running);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [listApps]);

  const options =
    value && !apps.some((app) => app.id === value.id) ? [value, ...apps] : apps;

  return (
    <section className="config-section" aria-labelledby="target-app-title">
      <div className="section-heading">
        <div>
          <h2 id="target-app-title">Target application</h2>
          <p>Keys are only sent while this application is frontmost.</p>
        </div>
      </div>

      <label className="target-app-field">
        <span>Restrict to application</span>
        <select
          disabled={disabled}
          onChange={(event) => {
            const id = event.currentTarget.value;
            const selected = options.find((app) => app.id === id);
            onChange(selected ? { id: selected.id, name: selected.name } : null);
          }}
          value={value?.id ?? ""}
        >
          <option value="">Any application</option>
          {options.map((app) => (
            <option key={app.id} value={app.id}>
              {app.name}
            </option>
          ))}
        </select>
      </label>
    </section>
  );
}
