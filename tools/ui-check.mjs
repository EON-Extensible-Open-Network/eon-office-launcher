// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Drives the real launcher window through the WebView2 DevTools protocol and
// checks what a person would see and do. No dependencies: Node 22+ has fetch
// and WebSocket built in. Key presses go to the webview through CDP, never to
// the operating system, so nothing else on the desktop receives them (the
// lesson EON Stream's v0.14 taught about SendKeys).
//
//   node tools/ui-check.mjs --launcher <exe> --core <exe> [--shots <dir>]
//
// Windows only (WebView2), and a debug build of the launcher: only debug
// builds open the DevTools port. Exit code 0 when every check passed.

import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const arg = (name) => {
  const at = process.argv.indexOf(name);
  return at > 0 ? process.argv[at + 1] : undefined;
};
const launcher = arg("--launcher");
const core = arg("--core");
const shots = arg("--shots");
if (!launcher || !core) {
  console.error("usage: node tools/ui-check.mjs --launcher <exe> --core <exe> [--shots <dir>]");
  process.exit(2);
}
if (shots) mkdirSync(shots, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const home = mkdtempSync(join(tmpdir(), "eon-launcher-ui-"));
const port = 9300 + Math.floor(Math.random() * 500);
let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(`  ${ok ? "ok  " : "FAIL"}  ${name}${ok || !detail ? "" : `: ${detail}`}`);
  if (!ok) failures++;
};

// ---- a protocol client for the owner and for a test application ---------------

function protocol(token, app) {
  const endpoint = JSON.parse(readFileSync(join(home, "runtime", "endpoint.json"), "utf8"));
  const [host, portText] = endpoint.address.split(":");
  const socket = createConnection({ host, port: Number(portText) });
  let buffer = "";
  let next = 1;
  const waiting = new Map();
  const handlers = [];
  socket.setEncoding("utf8");
  socket.on("data", (chunk) => {
    buffer += chunk;
    let at;
    while ((at = buffer.indexOf("\n")) >= 0) {
      const message = JSON.parse(buffer.slice(0, at));
      buffer = buffer.slice(at + 1);
      if (message.method) handlers.forEach((h) => h(message));
      else if (waiting.has(message.id)) {
        waiting.get(message.id)(message);
        waiting.delete(message.id);
      }
    }
  });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = next++;
    waiting.set(id, (m) => (m.error ? reject(m.error) : resolve(m.result)));
    socket.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  });
  const send = (message) => socket.write(JSON.stringify(message) + "\n");
  const ready = new Promise((resolve) => socket.on("connect", resolve))
    .then(() => call("session.hello", app ? { app, token } : { token }));
  return { call, send, ready, onMessage: (h) => handlers.push(h), close: () => socket.destroy() };
}

// ---- the DevTools connection ------------------------------------------------------

async function devtools() {
  for (let i = 0; i < 100; i++) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = pages.find((p) => p.type === "page" && p.webSocketDebuggerUrl);
      if (page) {
        const ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
        let next = 1;
        const waiting = new Map();
        ws.onmessage = (event) => {
          const message = JSON.parse(event.data);
          if (message.id && waiting.has(message.id)) {
            waiting.get(message.id)(message);
            waiting.delete(message.id);
          }
        };
        const send = (method, params = {}) => new Promise((resolve) => {
          const id = next++;
          waiting.set(id, resolve);
          ws.send(JSON.stringify({ id, method, params }));
        });
        return { send, close: () => ws.close() };
      }
    } catch { /* not up yet */ }
    await sleep(200);
  }
  throw new Error("the window's DevTools endpoint never appeared");
}

let cdp;
async function js(expression) {
  const reply = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (reply.result?.exceptionDetails) throw new Error(JSON.stringify(reply.result.exceptionDetails));
  return reply.result?.result?.value;
}
async function until(expression, ms = 8000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    // A page still loading throws; that is "not yet", not a failure.
    if (await js(expression).catch(() => false)) return true;
    await sleep(100);
  }
  return false;
}
async function key(code, keyName) {
  for (const type of ["keyDown", "keyUp"]) {
    await cdp.send("Input.dispatchKeyEvent", { type, key: keyName, code, windowsVirtualKeyCode: { ArrowDown: 40, ArrowUp: 38, End: 35, Home: 36 }[keyName] });
  }
}
async function shot(name) {
  if (!shots) return;
  const reply = await cdp.send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(shots, `${name}.png`), Buffer.from(reply.result.data, "base64"));
}

// ---- the run -------------------------------------------------------------------------

const idle = { command: "cmd", args: ["/C", "ping -n 60 127.0.0.1 >NUL"] };
const window = spawn(launcher, [], {
  env: {
    ...process.env,
    EON_OFFICE_HOME: home,
    EON_OFFICE_CORE_BIN: core,
    // Read by debug builds only (see main_window in src-tauri/src/main.rs).
    EON_WEBVIEW_ARGS: `--remote-debugging-port=${port}`,
  },
  stdio: "ignore",
});

let owner;
let tester;
try {
  console.log("EON Launcher interface check");
  cdp = await devtools();
  check("the core starts and the header says so", await until(`/0\\.1\\.\\d/.test(document.getElementById('core-status').textContent)`, 20000));
  check("the empty state explains how to install", await until(`!document.getElementById('apps-empty').hidden`));
  await shot("01-empty");

  owner = protocol(readFileSync(join(home, "runtime", "owner.token"), "utf8").trim());
  await owner.ready;
  await owner.call("prefs.set", { key: "locale", value: "en" });
  check("a language change from elsewhere reaches the window", await until(`document.documentElement.lang === 'en' && document.getElementById('tab-apps').textContent.trim() === 'Applications'`));
  check("after a language change the header still says the core is running", await until(`document.getElementById('core-status').textContent.startsWith('EON Office core')`));

  // A manifest on disk, reviewed through the window before anything is installed.
  const folder = join(home, "downloads");
  mkdirSync(folder, { recursive: true });
  const manifestPath = join(folder, "notes.json");
  writeFileSync(manifestPath, JSON.stringify({
    manifest: 0, id: "org.example.notes", name: "Example Notes", version: "1.0.0",
    api: { min: 0, max: 0 }, description: "Writes notes, shares them with search.",
    capabilities: ["notifications", "search.index", "storage"], provides: [{ method: "open" }],
  }));
  await js(`(() => { const i = document.getElementById('manifest-path'); i.value = ${JSON.stringify(manifestPath)}; document.querySelector('#install-form button[type=submit]').click(); })()`);
  check("review shows what it asks for, in words", await until(`document.getElementById('install-preview').textContent.includes('Send you notifications')`));
  const appsBefore = (await owner.call("apps.list")).apps.length;
  check("reviewing installs nothing", appsBefore === 0);
  await shot("02-review");
  await js(`[...document.querySelectorAll('#install-preview button')].find(b => b.textContent === 'Install').click()`);
  check("installing shows the card", await until(`document.querySelectorAll('#apps > li').length === 1`));
  check("nothing is allowed after installing", await until(`[...document.querySelectorAll('#apps .perms button')].every(b => b.getAttribute('aria-pressed') === 'false')`));
  check("the removal question stays hidden until asked", await js(`[...document.querySelectorAll('#apps p')].every(p => !p.textContent.includes('Remove') || p.offsetParent === null)`));

  // Allow two permissions by clicking, and confirm in the core.
  for (const wording of ["Send you notifications", "Make its items searchable"]) {
    await js(`[...document.querySelectorAll('#apps .perms li')].find(li => li.textContent.includes(${JSON.stringify(wording)})).querySelector('button').click()`);
    await sleep(300);
  }
  const granted = (await owner.call("apps.list")).apps[0].granted.sort();
  check("clicking Allow grants exactly that permission", JSON.stringify(granted) === JSON.stringify(["notifications", "search.index"]), JSON.stringify(granted));
  await shot("03-permissions");

  // The application itself connects (this script plays it), posts and indexes.
  const notesToken = readFileSync(join(home, "apps", "org.example.notes", "token"), "utf8").trim();
  tester = protocol(notesToken, "org.example.notes");
  await tester.ready;
  tester.onMessage((m) => {
    if (m.method === "invoke") tester.send({ jsonrpc: "2.0", id: m.id, result: { opened: m.params.params.id } });
  });
  await tester.call("search.index", { id: "n1", title: "İstanbul toplantısı", text: "Bütçe ve takvim", visibility: "shared" });
  await tester.call("notifications.post", { title: "Note saved", body: "İstanbul toplantısı" });
  check("a connected app shows as running", await until(`document.querySelector('#apps .state').textContent === 'Running'`));
  check("a notification raises the badge", await until(`document.getElementById('unread-badge').textContent === '1' && !document.getElementById('unread-badge').hidden`));
  check("the notification is announced to screen readers", await until(`document.getElementById('announcer').textContent.includes('Note saved')`));

  // Keyboard: arrows move between sections.
  await js(`document.getElementById('tab-apps').focus()`);
  await key("ArrowDown", "ArrowDown");
  check("ArrowDown moves to the next section", await until(`document.activeElement.id === 'tab-market' && !document.getElementById('panel-market').hidden`));
  await key("End", "End");
  check("End moves to the last section", await until(`document.activeElement.id === 'tab-activity'`));
  await key("Home", "Home");
  check("Home moves to the first section", await until(`document.activeElement.id === 'tab-apps'`));

  // Search, typed the way a keyboard without Turkish letters would.
  await js(`document.getElementById('tab-search').click()`);
  await js(`(() => { const q = document.getElementById('search-query'); q.value = 'ISTANBUL'; q.dispatchEvent(new Event('input')); })()`);
  check("the result count is a sentence", await until(`document.getElementById('search-status').textContent === '1 found'`));
  check("search finds Turkish text typed without Turkish letters", await until(`document.querySelectorAll('#search-results li').length === 1 && document.getElementById('search-results').textContent.includes('İstanbul')`));
  await js(`document.querySelector('#search-results button').click()`);
  await sleep(500);
  check("Open asks the owning application, which answered", !(await js(`document.getElementById('announcer').textContent.includes('cannot')`)));
  await shot("04-search");

  // Notifications.
  await js(`document.getElementById('tab-notifications').click()`);
  check("the notification is listed", await until(`document.getElementById('notification-list').textContent.includes('Note saved')`));
  await js(`[...document.querySelectorAll('#notification-list button')].find(b => b.textContent === 'Mark as read').click()`);
  check("marking read clears the badge", await until(`document.getElementById('unread-badge').hidden`));
  await shot("05-notifications");

  // Preferences: applied to the launcher itself, and stored for every app.
  await js(`document.getElementById('tab-preferences').click()`);
  await js(`(() => { const r = document.getElementById('pref-font-scale'); r.value = '1.5'; r.dispatchEvent(new Event('change')); })()`);
  check("text size applies to the launcher", await until(`getComputedStyle(document.documentElement).getPropertyValue('--scale').trim() === '1.5'`));
  check("text size is stored for every app", (await owner.call("prefs.get")).font_scale === 1.5);
  await js(`(() => { const c = document.getElementById('pref-high-contrast'); c.checked = true; c.dispatchEvent(new Event('change')); })()`);
  check("high contrast applies", await until(`document.documentElement.dataset.contrast === 'high'`));
  await shot("06-preferences");
  await js(`(() => { const c = document.getElementById('pref-high-contrast'); c.checked = false; c.dispatchEvent(new Event('change')); const r = document.getElementById('pref-font-scale'); r.value = '1'; r.dispatchEvent(new Event('change')); })()`);

  // Activity record.
  await js(`document.getElementById('tab-activity').click()`);
  check("the activity record is shown and intact", await until(`document.getElementById('verify-status').textContent.startsWith('The record is intact')`));
  await shot("07-activity");

  // The marketplace: previews off shows why nothing installs; on, an unsigned
  // install needs its own button and states that it is unsigned.
  await js(`document.getElementById('tab-market').click()`);
  check("the index is read and explains a preview-only app", await until(`document.getElementById('market-list').textContent.includes('no stable release')`, 20000));
  await js(`(() => { const c = document.getElementById('market-previews'); c.checked = true; c.dispatchEvent(new Event('change')); })()`);
  check("with previews, EON Hello offers an unsigned install and says so", await until(`[...document.querySelectorAll('#market-list > li')].some(li => li.querySelector('h2').textContent === 'EON Hello' && li.textContent.includes('Not signed') && [...li.querySelectorAll('button')].some(b => b.textContent === 'Install unsigned'))`, 20000));
  check("a permission to use another app names it, installed or not", await js(`document.getElementById('market-list').textContent.includes('Use EON Hello')`));
  await shot("07b-market");
  await js(`[...document.querySelectorAll('#market-list > li')].find(li => li.querySelector('h2').textContent === 'EON Hello').querySelector('button').click()`);
  check("installing from the index downloads, checks and installs", await until(`document.getElementById('market-status').textContent.startsWith('EON Hello is installed')`, 60000), await js(`document.getElementById('market-status').textContent`));
  const fromMarket = (await owner.call("apps.list")).apps.find((a) => a.id === "org.eon.office.hello");
  check("an app from the index is installed with nothing granted", !!fromMarket && fromMarket.granted.length === 0, JSON.stringify(fromMarket?.granted));
  check("its card then says Installed", await until(`[...document.querySelectorAll('#market-list > li')].some(li => li.querySelector('h2').textContent === 'EON Hello' && li.textContent.includes('Installed'))`));

  // Every control has a name a screen reader can read.
  const unnamed = await js(`[...document.querySelectorAll('button, input, select')].filter(e => {
    if (e.closest('[hidden]')) return false;
    const label = e.getAttribute('aria-label') || e.textContent.trim() || (e.id && document.querySelector('label[for="' + e.id + '"]')?.textContent.trim());
    return !label;
  }).map(e => e.outerHTML.slice(0, 80))`);
  check("every visible control has an accessible name", unnamed.length === 0, JSON.stringify(unnamed));

  // A launchable app: Start, then Stop, from the window.
  await owner.call("apps.install", { manifest: { manifest: 0, id: "org.example.idle", name: "Idle", version: "1.0.0", api: { min: 0, max: 0 }, entry: idle } });
  await js(`document.getElementById('tab-apps').click()`);
  const idleCard = `[...document.querySelectorAll('#apps > li')].find(li => li.querySelector('h2').textContent === 'Idle')`;
  check("a launchable app offers Start", await until(`(${idleCard})?.querySelector('.actions button').textContent === 'Start'`));
  await js(`${idleCard}.querySelector('.actions button').click()`);
  const started = await (async () => { for (let i = 0; i < 50; i++) { const s = await owner.call("apps.status", { app: "org.example.idle" }); if (s.process?.state === "running") return true; await sleep(100); } return false; })();
  check("Start launches it through the core", started);
  await owner.call("apps.stop", { app: "org.example.idle" });

  // Remove asks first, then removes.
  await js(`[...document.querySelectorAll('#apps > li')].find(li => li.querySelector('h2').textContent === 'Example Notes').querySelector('button.danger').click()`);
  check("Remove asks before removing", await until(`[...document.querySelectorAll('#apps p')].some(p => p.textContent.startsWith('Remove Example Notes?') && p.offsetParent !== null)`));
  await js(`[...document.querySelectorAll('#apps > li')].find(li => li.querySelector('h2').textContent === 'Example Notes').querySelectorAll('button.danger')[1].click()`);
  check("confirming removes it", await until(`![...document.querySelectorAll('#apps h2')].some(h => h.textContent === 'Example Notes')`));
  await shot("08-removed");
} catch (error) {
  failures++;
  console.error(`  FAIL  ${error.stack || error}`);
} finally {
  try { tester?.close(); } catch { /* gone */ }
  // Stop the core the launcher started, even when the run failed before the
  // owner connection existed -- otherwise it outlives the check.
  try {
    if (!owner && existsSync(join(home, "runtime", "endpoint.json"))) {
      owner = protocol(readFileSync(join(home, "runtime", "owner.token"), "utf8").trim());
      await owner.ready;
    }
    await owner?.call("core.shutdown");
  } catch { /* already gone */ }
  owner?.close();
  cdp?.close();
  window.kill();
  await sleep(500);
  try { rmSync(home, { recursive: true, force: true }); } catch { /* still in use */ }
}
console.log(failures ? `\n${failures} check(s) failed.` : "\nall checks passed");
process.exit(failures ? 1 : 0);
