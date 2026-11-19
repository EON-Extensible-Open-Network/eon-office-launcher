**EON Launcher** — the one window for EON Office, and the **EON Office core** it runs on.
Protocol `v0`: early, unstable, for trying out and for building applications against.

**Install:** put both executables in one folder and rename them to `eon-office-launcher.exe`
and `eon-office-core.exe`, then start the launcher. It starts the core for you. Check the
files against `SHA256SUMS.txt` first.

**These builds are not signed.** EON has no signing keys yet, deliberately: they belong to
a legal entity that does not exist yet (plan, madde 30 and 34). Windows SmartScreen will
warn, and **Smart App Control, if it is on, will refuse to run them** — that is Windows
doing its job with an unsigned download, not a fault in the build.

What it does, and what it does not do yet: see the
[README](https://github.com/EON-Extensible-Open-Network/eon-office-launcher#readme) and the
[changelog](https://github.com/EON-Extensible-Open-Network/eon-office-launcher/blob/main/CHANGELOG.md).
No telemetry, no account, nothing leaves your device.
