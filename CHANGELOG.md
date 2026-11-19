<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Changelog

## 0.2.0 — install from the marketplace

- **Get applications:** reads the EON Office index (eon-office-marketplace), shows each
  application's newest release this machine can run — or why none — with what it asks for,
  its publisher and licence, and whether it is installed or has an update.
- Installing downloads the file over https only, compares its size and SHA-256 with the
  index, checks the signature (madde 34 format; every release today is unsigned and needs
  its own "Install unsigned" button), saves it where the core looks for it, and installs its
  manifest with nothing granted. Requirements are installed first.
- Previews are opt-in; the choice is stored in the core.
- `tools/ui-check.mjs`: 36 checks, including an install from the live index.

## 0.1.0 — first version

- Applications: review a manifest before installing, install with nothing granted,
  allow and take back each permission in plain words, start, stop, remove with a question
  first. Drop a manifest on the window to review it.
- Search across applications with Turkish-aware matching; *Open* through the well-known
  `open` method.
- Notifications from every application, announced to screen readers, with an unread badge.
- Preferences for every application: language (English, Türkçe, system), theme, reduced
  motion, high contrast, text size — applied to the launcher too.
- Activity: the core's audit record with an integrity check.
- Starts the core when it is not running; stops it on exit only if it started it and no
  application is connected.
- `--selftest`: strings complete in both languages, static interface checks, a real core
  started, queried and stopped.
- `tools/ui-check.mjs`: 30 checks driving the real window through DevTools.
