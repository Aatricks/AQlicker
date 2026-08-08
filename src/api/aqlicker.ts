import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppConfig, LogicalKey, Mode } from "../domain/config";

export const RUN_STATE_EVENT = "aqlicker://run-state";

export type RunStatus = "idle" | "running" | "stopping" | "failed";
export type StopReason =
  | "requested"
  | "durationComplete"
  | "inputFailure"
  | "workerPanic";

export interface RunError {
  code: string;
  key: LogicalKey | null;
  message: string;
}

export interface RunSnapshot {
  status: RunStatus;
  mode: Mode | null;
  elapsedMs: number;
  remainingMs: number | null;
  successfulPresses: number;
  /** A restricted run stays `running` while its target application is away. */
  paused: boolean;
  waitingForApp: string | null;
  stopReason: StopReason | null;
  error: RunError | null;
}

export interface RunningApp {
  id: string;
  name: string;
}

export interface PermissionStatus {
  granted: boolean;
  sameIntegrityOnly: boolean;
}

export interface ShortcutRegistrationStatus {
  shortcut: string;
  registered: boolean;
  error: string | null;
}

export interface BootstrapPayload {
  config: AppConfig;
  recoveryNotice: { code: string } | null;
  permission: PermissionStatus;
  shortcut: ShortcutRegistrationStatus;
  run: RunSnapshot;
}

export interface AqlickerApi {
  bootstrap(): Promise<BootstrapPayload>;
  saveConfig(config: AppConfig): Promise<void>;
  startRun(config: AppConfig): Promise<RunSnapshot>;
  stopRun(): Promise<RunSnapshot>;
  requestAccess(): Promise<PermissionStatus>;
  permissionStatus(): Promise<PermissionStatus>;
  setShortcut(shortcut: string): Promise<string>;
  listApps(): Promise<RunningApp[]>;
  listenRunState(handler: (state: RunSnapshot) => void): Promise<() => void>;
}

export const aqlickerApi: AqlickerApi = {
  bootstrap: () => invoke<BootstrapPayload>("bootstrap"),
  saveConfig: (config) => invoke<void>("save_config", { config }),
  startRun: (config) => invoke<RunSnapshot>("start_run", { config }),
  stopRun: () => invoke<RunSnapshot>("stop_run"),
  requestAccess: () => invoke<PermissionStatus>("request_access"),
  permissionStatus: () => invoke<PermissionStatus>("permission_status"),
  setShortcut: (shortcut) => invoke<string>("set_shortcut", { shortcut }),
  listApps: () => invoke<RunningApp[]>("list_apps"),
  listenRunState: (handler) =>
    listen<RunSnapshot>(RUN_STATE_EVENT, (event) => handler(event.payload)),
};
