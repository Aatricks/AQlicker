# AQlicker

A key spammer for macOS and Windows. You pick a list of keys, and AQlicker presses them for you, either at a fixed interval or with timing that varies the way a person's would.

## Requirements

**macOS needs Accessibility permission.** Without it AQlicker cannot send key events, and Start stays disabled. Grant it under System Settings → Privacy & Security → Accessibility, then return to the app.

The permission is tied to the exact binary. Unsigned builds get a new identity every time you rebuild, so you will usually have to grant it again after each `tauri build`.

On Windows no permission is needed for normal applications. Input into an elevated window is blocked unless AQlicker runs elevated too. AQlicker never elevates itself.

## Building

```bash
pnpm install
pnpm tauri build
```

For development:

```bash
pnpm tauri dev
```

Builds are unsigned and not notarized. macOS Gatekeeper will complain, and Windows SmartScreen will warn.

## Modes

**Timer** walks the key list in order at a fixed interval, from 40 ms to 60 s.

**Natural** picks keys at random using per-key weights from 1 to 10, so a key with weight 3 comes up about three times as often as one with weight 1. Intervals vary, with occasional short bursts and pauses. One slider handles this for most cases; an advanced section exposes the minimum and maximum interval, burst intensity, and pause chance (capped at 25%).

Both modes accept an optional stop-after duration, from 1 second to 24 hours.

## Stopping a run

- The global shortcut, `Cmd/Ctrl+Shift+K` by default, toggles start and stop from any application.
- `Escape` stops a run while one is active. It does nothing otherwise.
- Closing the window cancels the run and releases any key still held.

AQlicker only emits letters, digits, function keys, arrows, Space, Enter, Tab, and common punctuation. It will not send modifiers or key combinations, so it cannot produce a system shortcut.

## Restricting to one application

You can name a target application. While it is frontmost, keys go out normally. When you switch away, the run pauses, releases any held key, and shows what it is waiting for. Switching back resumes it. Time spent paused still counts toward the stop-after duration.

On macOS this reads the frontmost window through `CGWindowListCopyWindowInfo`, which needs no permission beyond the Accessibility grant AQlicker already requires. Two known limits: an application whose windows are all minimized owns no on-screen window, so the run stays paused; and Stage Manager's own window is skipped explicitly, since it otherwise sits in front of everything.

## Configuration

Settings save automatically to a local file and reload on launch. A run never resumes by itself. If the file is unreadable, AQlicker keeps it as a backup and starts from defaults rather than overwriting it.

## Testing input

`tools/input-target/index.html` is a page that logs every keydown and keyup with its code, a timestamp, and which keys are still down. Open it in a browser to check what AQlicker is actually sending, including whether anything was left held.

## Status

The macOS build works and has been used. Windows compiles and is covered by the same tests, but has not been run on real hardware; the application list in particular will likely need work there.

macOS Spaces are not supported. There is no public API for the current Space, and the private ones break across OS releases.
