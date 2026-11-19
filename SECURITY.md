<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Security

Report privately through GitHub's **Security → Report a vulnerability** on this repository;
the organisation-wide policy is
[`.github/SECURITY.md`](https://github.com/EON-Extensible-Open-Network/.github/blob/main/SECURITY.md).

What matters here:

- **The launcher holds the owner's token.** Owner authority is everything: install,
  grant, remove, read the audit log. The token stays in the host process; the interface
  (the webview) never receives it and has no command that returns it.
- **Manifests are untrusted input.** A manifest's name, description and capabilities are
  shown before installing so the person decides with the facts; they enter the page only
  through `textContent`, never as markup, and the CSP forbids inline script.
- **The interface has no network.** No plugin is enabled (no file system, shell or HTTP);
  CI fails the build if `fetch`, `XMLHttpRequest`, `WebSocket` or `EventSource` appear in
  `src/`.
- **The DevTools port is debug-only.** `EON_WEBVIEW_ARGS` is read only by debug builds;
  a release build ignores it.
- **Nothing is granted by installing**, and removing an application asks first, because
  it deletes the application's data.

The core's threat model:
[`eon-office-core/SECURITY.md`](https://github.com/EON-Extensible-Open-Network/eon-office-core/blob/main/SECURITY.md).
