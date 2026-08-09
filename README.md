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

## Presets

Settings live in named presets. A preset holds the key list with its weights and cooldowns, the mode, the timer interval, the natural settings including the advanced overrides, the automatic stop, and the target application. The global shortcut is not part of a preset; it is a single app-level setting, so switching presets does not change how a run is started or stopped.

One preset is active at a time, and Start uses it. Edits save into the active preset as you make them, the same way settings have always saved. There is no Save button.

A second, optional global shortcut switches to the next preset, wrapping around from the last back to the first. It starts unassigned, so upgrading never takes a hotkey you already use elsewhere; record one under **Preset cycling shortcut**, and clear it there again. With a single preset it does nothing. While a run is active it is refused, like every other configuration change.

The preset control at the top of the panel selects the active preset and creates, duplicates, renames and deletes presets. A name is trimmed, cannot be empty, and is limited to 60 characters. Two presets may share a name. There is always at least one preset, so deleting the last one is refused. While a run is active the whole configuration is locked, including switching and editing presets.

Configuration files written by an earlier version are read as a single preset named "Default".

## Menu bar

AQlicker puts an item in the macOS menu bar. Its menu starts or stops a run, lists the presets with the active one ticked, shows the window, and quits. The labels follow the application: the first entry reads Start or Stop, and the tick moves when the preset changes, however it changed. While a run is active the preset entries are greyed out, since the configuration is locked; Start/Stop and Quit stay available.

Quitting from the menu goes through the same shutdown as closing the window: the run is cancelled, any held key released, and the shortcuts unregistered before the application exits. The menu bar item disappears with it.

## Modes

**Timer** walks the key list in order at a fixed interval, from 40 ms to 60 s.

**Natural** picks keys at random using per-key weights from 1 to 10, so a key with weight 3 comes up about three times as often as one with weight 1. Intervals vary, with occasional short bursts and pauses. One slider handles this for most cases; an advanced section exposes the minimum and maximum interval, burst intensity, and pause chance (capped at 25%).

Each key in natural mode also has a cooldown, from 0 to 60,000 ms. It is 0 by default, which means no cooldown. After a key is pressed, it is not picked again until its cooldown has elapsed. The other keys stay available while it cools, and their weights are shared out among them. When every selected key is cooling, the run waits for the first one to come back and presses that key, so a cooldown is never cut short. Waiting still counts toward the stop-after duration, and a cooldown longer than the remaining duration ends the run on its deadline with no key held. Cooldowns run against the clock, so time the run spends paused waiting for a target application counts toward them. Timer mode is unaffected.

Both modes accept an optional stop-after duration, from 1 second to 24 hours.

## Stopping a run

- The global shortcut, `Cmd/Ctrl+Shift+K` by default, toggles start and stop from any application.
- Stop from the menu bar item, or quit from it.
- `Escape` stops a run while one is active. It does nothing otherwise.
- Closing the window cancels the run and releases any key still held.

AQlicker only emits letters, digits, function keys, arrows, Space, Enter, Tab, and common punctuation. It will not send modifiers or key combinations, so it cannot produce a system shortcut.

## Restricting to one application

You can name a target application. While it is frontmost, keys go out normally. When you switch away, the run pauses, releases any held key, and shows what it is waiting for. Switching back resumes it. Time spent paused still counts toward the stop-after duration.

On macOS this reads the frontmost window through `CGWindowListCopyWindowInfo`, which needs no permission beyond the Accessibility grant AQlicker already requires. Two known limits: an application whose windows are all minimized owns no on-screen window, so the run stays paused; and Stage Manager's own window is skipped explicitly, since it otherwise sits in front of everything.

## Configuration

Settings save automatically to a local file and reload on launch. The file holds every preset, which one is active, and the two app-level shortcuts. A run never resumes by itself. If the file is unreadable, AQlicker keeps it as a backup and starts from defaults rather than overwriting it.

## Testing input

`tools/input-target/index.html` is a page that logs every keydown and keyup with its code, a timestamp, and which keys are still down. Open it in a browser to check what AQlicker is actually sending, including whether anything was left held.

## Status

The macOS build works and has been used. Windows compiles and is covered by the same tests, but has not been run on real hardware; the application list in particular will likely need work there.

macOS Spaces are not supported. There is no public API for the current Space, and the private ones break across OS releases.
