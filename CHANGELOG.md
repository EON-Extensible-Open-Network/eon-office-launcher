<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Changelog

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
