<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 EON contributors
-->

# Contributing to EON Launcher

The organisation-wide guide,
[`.github/CONTRIBUTING.md`](https://github.com/EON-Extensible-Open-Network/.github/blob/main/CONTRIBUTING.md),
applies: DCO sign-off (`git commit -s`), Conventional Commits (`feat(launcher): ...`).
By contributing you license your work under `GPL-3.0-or-later` with the EON Module ABI
Exception (`LICENSE-EXCEPTION.md`).

- **Every string in both languages.** Add it to `src-tauri/i18n/en.json` and `tr.json`
  with the same `{placeholders}`; `--selftest` fails otherwise.
- **Every control has a name and works from the keyboard.** `checks.rs` and
  `tools/ui-check.mjs` check what a machine can; say in the pull request how you checked
  the rest.
- **No new IPC command without a reason.** The surface is `src-tauri/src/main.rs`'s
  handler list; each command is something the interface cannot do otherwise.
- **Untrusted text through `textContent` only.** Never `innerHTML` with anything that came
  from an application or a manifest.
- **The SDK and core are pinned by commit.** To build against unpushed local changes, pass
  a patch on the command line after the subcommand, as in EON Stream:
  `cargo build --config 'patch."https://github.com/EON-Extensible-Open-Network/eon-office-sdk".eon-office-sdk.path="../eon-office-sdk"'`
  and `git checkout Cargo.lock` before committing.
