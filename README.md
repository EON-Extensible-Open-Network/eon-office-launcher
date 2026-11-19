<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# EON Launcher

**The one window for EON Office.** Install an application by dropping its manifest onto
the window, see in plain words what it asks to do, allow or take back each permission,
start and stop it, and find everything in one place: notifications from every application,
search across all of them, the preferences every application follows, and the record of
every change of who may do what.

It is the first application of [EON Office](https://github.com/EON-Extensible-Open-Network/eon-docs/blob/main/office/roadmap.md)
(Wave 0) and the person's shell for the
[EON Office core](https://github.com/EON-Extensible-Open-Network/eon-office-core): it
connects as the **owner**, and starts the core if it is not running. It is a separate
program, like every EON Office application, and is not required — everything it does is
in the [protocol](https://github.com/EON-Extensible-Open-Network/eon-office-spec/blob/main/docs/protocol.md).

> **0.1.0 — first version.** Windows is where it is built and checked end to end; macOS and
> Linux build in CI but their windows have not been looked at yet.

## What it does

| Section | |
|---|---|
| **Applications** | Review a manifest before installing (nothing is installed by reviewing); install without granting anything; allow or take back each permission, worded as `eon-office-spec/docs/capabilities.md` words it; start, stop; remove — with a question first, because removing deletes the application's data. |
| **Get applications** | The EON Office marketplace index: what each release asks for, who publishes it, its licence, and — today, for every release — that it is **unsigned**, with its own button to install anyway. The file is downloaded and its size and SHA-256 compared with the index before anything is installed; nothing is granted by installing. Previews are opt-in. |
| **Recent** | What you created, opened, edited or shared in your applications — titles only, newest first, on this device — from the well-known `activity` event; *Open* goes back to it. `eon-office://` links open items the same way. |
| **Search** | One box over every application's shared items. Turkish letters fold: `ISTANBUL` finds "İstanbul". *Open* asks the owning application (the well-known `open` method). |
| **Notifications** | Every application's notifications; announced to screen readers as they arrive; mark as read. |
| **Preferences** | Language (English, Türkçe, or the system's), theme, reduced motion, high contrast, text size — stored in the core and applied to every application at once, the launcher included. |
| **Activity** | The core's audit record, newest first, with a check that it has not been altered. |

Closing the launcher stops the core only if the launcher started it and no application is
using it.

## Accessibility (madde 35)

Keyboard throughout: sections are a tab list (arrows, Home, End), every control is a real
button or field with a name, focus is always visible. Screen readers hear status changes
and new notifications through live regions. Every text colour pair meets WCAG AA in the
light and dark themes and AAA in high contrast; sizes are in `rem`, so the text-size
preference scales the whole interface; reduced motion turns transitions off. These are
checked by machine (`--selftest`, `tools/ui-check.mjs`); a session with a real screen reader
is still owed before `1.0`.

## Run

Put `eon-office-core` beside `eon-office-launcher` (or on `PATH`, or name it with
`EON_OFFICE_CORE_BIN`) and start the launcher. It uses the core's home directory
(`EON_OFFICE_HOME`, else the per-user data directory).

```
eon-office-launcher                    open the window
eon-office-launcher --selftest [FILE]  strings, interface, and a real core started and stopped
eon-office-launcher --version
```

## Develop

The interface is static HTML, CSS and JavaScript in `src/` — no Node, no bundler, as in
EON Stream. The host is `src-tauri/`: a library (`host`, `checks`) and the window over it.

```
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
EON_OFFICE_CORE_BIN=/path/to/eon-office-core cargo test -- --include-ignored
eon-office-launcher --selftest
node tools/ui-check.mjs --launcher target/debug/eon-office-launcher.exe --core /path/to/eon-office-core.exe --shots shots
```

`tools/ui-check.mjs` (Node 22+, no dependencies, Windows) opens the real window of a
**debug** build with a DevTools port (`EON_WEBVIEW_ARGS`, ignored by release builds),
plays an application itself, and drives the interface through 37 checks: review, install,
permissions, notifications, keyboard navigation, search, open, preferences, activity,
installing from the live marketplace index, start, remove. Keys go to the webview through the DevTools protocol, never to the
operating system.

Licence `GPL-3.0-or-later` with the [EON Module ABI Exception](LICENSE-EXCEPTION.md).
Contributing: [CONTRIBUTING.md](CONTRIBUTING.md). Security: [SECURITY.md](SECURITY.md).
