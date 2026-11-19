// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// EON Launcher's interface. It has no network of its own: everything goes
// through the host's commands (invoke). Everything that comes from an
// application -- names, titles, notification text -- is untrusted, and enters
// the page only through textContent, never as markup.

"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
let strings = {};
let appNames = {};
let prefs = {};
// What the header should say; re-rendered whenever the language changes.
let coreState = { key: "core.connecting", vars: {}, error: false };

function showCore(key, vars = {}, error = false) {
  coreState = { key, vars, error };
  const node = $("core-status");
  node.textContent = t(key, vars);
  node.classList.toggle("error", error);
  $("retry").hidden = !error;
}

// ---- strings -----------------------------------------------------------------

function t(key, vars = {}) {
  let text = strings[key] ?? key;
  for (const [name, value] of Object.entries(vars)) {
    text = text.replaceAll(`{${name}}`, String(value));
  }
  return text;
}

function languageOf(locale) {
  if (locale && locale !== "system") return locale.startsWith("tr") ? "tr" : "en";
  return (navigator.language || "en").toLowerCase().startsWith("tr") ? "tr" : "en";
}

async function loadStrings(locale) {
  const language = languageOf(locale);
  strings = await invoke("strings", { language });
  document.documentElement.lang = language;
  document.title = t("app.title");
  for (const node of document.querySelectorAll("[data-i18n]")) {
    node.textContent = t(node.dataset.i18n);
  }
  for (const node of document.querySelectorAll("[data-i18n-label]")) {
    node.setAttribute("aria-label", t(node.dataset.i18nLabel));
  }
  for (const node of document.querySelectorAll("[data-i18n-placeholder]")) {
    node.setAttribute("placeholder", t(node.dataset.i18nPlaceholder));
  }
}

function announce(text) {
  const region = $("announcer");
  region.textContent = "";
  // A fresh node each time, so the same text twice is still announced.
  window.setTimeout(() => { region.textContent = text; }, 30);
}

function errorText(error) {
  const reason = (error && (error.message || error.code)) || String(error);
  return t("error.generic", { reason });
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value);
  }
  for (const child of children) if (child) node.append(child);
  return node;
}

// ---- preferences, applied to the launcher itself ------------------------------

function applyPrefs(p) {
  prefs = p;
  const root = document.documentElement;
  if (p.theme === "light" || p.theme === "dark") root.dataset.theme = p.theme;
  else delete root.dataset.theme;
  if (p.high_contrast) root.dataset.contrast = "high"; else delete root.dataset.contrast;
  if (p.reduce_motion) root.dataset.motion = "reduce"; else delete root.dataset.motion;
  root.style.setProperty("--scale", String(p.font_scale || 1));
  $("pref-locale").value = p.locale || "system";
  $("pref-theme").value = ["light", "dark"].includes(p.theme) ? p.theme : "system";
  $("pref-reduce-motion").checked = !!p.reduce_motion;
  $("pref-high-contrast").checked = !!p.high_contrast;
  $("pref-font-scale").value = String(p.font_scale || 1);
  $("font-scale-value").textContent = `${Math.round((p.font_scale || 1) * 100)}%`;
}

async function setPref(key, value) {
  try {
    await invoke("set_pref", { key, value });
    $("prefs-status").textContent = t("prefs.saved");
  } catch (error) {
    $("prefs-status").textContent = errorText(error);
  }
}

// ---- tabs: arrows move, Home/End jump, Enter/Space activate ---------------------

const tabs = () => Array.from(document.querySelectorAll('[role="tab"]'));

function selectTab(tab, focus = true) {
  for (const other of tabs()) {
    const selected = other === tab;
    other.setAttribute("aria-selected", String(selected));
    other.tabIndex = selected ? 0 : -1;
    $(other.getAttribute("aria-controls")).hidden = !selected;
  }
  if (focus) tab.focus();
  refresh(tab.id);
}

$("tabs").addEventListener("keydown", (event) => {
  const list = tabs();
  const at = list.indexOf(document.activeElement);
  if (at < 0) return;
  const moves = { ArrowDown: at + 1, ArrowRight: at + 1, ArrowUp: at - 1, ArrowLeft: at - 1, Home: 0, End: list.length - 1 };
  if (event.key in moves) {
    event.preventDefault();
    selectTab(list[(moves[event.key] + list.length) % list.length]);
  }
});
for (const tab of tabs()) tab.addEventListener("click", () => selectTab(tab));

function refresh(tabId) {
  if (tabId === "tab-apps") renderApps();
  if (tabId === "tab-market") renderMarket();
  if (tabId === "tab-notifications") renderNotifications();
  if (tabId === "tab-activity") renderActivity();
}

const currentTab = () => tabs().find((tab) => tab.getAttribute("aria-selected") === "true");

// ---- applications -----------------------------------------------------------

function capabilityText(capability) {
  if (capability.startsWith("call:")) {
    const id = capability.slice(5);
    return t("cap.call", { name: appNames[id] || id });
  }
  return t(`cap.${capability}`);
}

function stateOf(app) {
  if (app.connected) return ["running", t("apps.running")];
  const state = app.process && app.process.state;
  if (state === "crashed") return ["crashed", t("apps.crashed")];
  if (state === "failed") return ["failed", t("apps.failed")];
  return ["stopped", t("apps.stopped")];
}

async function act(promise, after = renderApps) {
  try {
    await promise;
  } catch (error) {
    announce(errorText(error));
  }
  await after();
}

function appCard(app) {
  const [stateClass, stateText] = stateOf(app);
  const actions = el("div", { class: "actions" });
  if (app.launchable) {
    if (app.connected) actions.append(el("button", { class: "secondary", text: t("apps.stop"), onclick: () => act(invoke("stop", { app: app.id })) }));
    else actions.append(el("button", { text: t("apps.launch"), onclick: () => act(invoke("launch", { app: app.id })) }));
  } else {
    actions.append(el("span", { class: "muted state", text: t("apps.not.launchable") }));
  }
  const confirmRow = el("div", { class: "actions", hidden: "" });
  const remove = el("button", { class: "danger", text: t("apps.uninstall") });
  remove.addEventListener("click", () => {
    confirmRow.hidden = false;
    confirmRow.querySelector("button").focus();
  });
  confirmRow.append(
    el("p", { text: t("apps.uninstall.confirm", { name: app.name }) }),
    el("button", { class: "danger", text: t("apps.uninstall"), onclick: () => act(invoke("uninstall", { app: app.id })) }),
    el("button", { class: "secondary", text: t("apps.install.cancel"), onclick: () => { confirmRow.hidden = true; remove.focus(); } }),
  );
  actions.append(remove);

  const requested = [...app.granted, ...app.pending];
  const perms = el("ul", { class: "perms", "aria-label": `${t("apps.permissions")}: ${app.name}` });
  for (const capability of requested.sort()) {
    const allowed = app.granted.includes(capability);
    const toggle = el("button", {
      class: allowed ? "" : "secondary",
      "aria-pressed": String(allowed),
      text: allowed ? t("apps.permission.allowed") : t("apps.permission.allow"),
      title: allowed ? t("apps.permission.revoke") : t("apps.permission.allow"),
      "aria-label": `${capabilityText(capability)}: ${allowed ? t("apps.permission.allowed") : t("apps.permission.allow")}`,
      onclick: () => act(invoke("permission", { app: app.id, capability, allowed: !allowed })),
    });
    perms.append(el("li", {}, el("span", { text: capabilityText(capability) }), toggle));
  }

  return el("li", { class: "card" },
    el("header", {},
      el("h2", { text: app.name }),
      el("span", { class: "muted", text: t("apps.version", { version: app.version }) }),
      el("span", { class: `state ${stateClass}`, text: stateText }),
    ),
    app.description ? el("p", { class: "muted", text: app.description }) : null,
    requested.length ? perms : null,
    actions,
    confirmRow,
  );
}

async function renderApps() {
  let list = [];
  try {
    list = (await invoke("apps")).apps || [];
  } catch (error) {
    $("core-status").textContent = errorText(error);
    return;
  }
  appNames = Object.fromEntries(list.map((app) => [app.id, app.name]));
  $("apps-empty").hidden = list.length > 0;
  $("apps").replaceChildren(...list.map(appCard));
}

async function showPreview(path) {
  const box = $("install-preview");
  try {
    const { manifest, path: file } = await invoke("preview", { path });
    const caps = manifest.capabilities || [];
    const asks = caps.length
      ? el("ul", {}, ...caps.map((c) => el("li", { text: capabilityText(c) })))
      : el("p", { text: t("apps.install.asks.nothing") });
    box.replaceChildren(
      el("h2", { text: `${manifest.name} ${manifest.version}` }),
      manifest.description ? el("p", { class: "muted", text: manifest.description }) : null,
      caps.length ? el("p", { text: t("apps.install.asks") }) : null,
      asks,
      el("p", { class: "muted", text: t("apps.install.note") }),
      el("div", { class: "actions" },
        el("button", { text: t("apps.install.confirm"), onclick: async () => {
          box.hidden = true;
          $("manifest-path").value = "";
          await act(invoke("install", { path: file }));
        } }),
        el("button", { class: "secondary", text: t("apps.install.cancel"), onclick: () => { box.hidden = true; $("manifest-path").focus(); } }),
      ),
    );
    box.hidden = false;
    box.focus();
  } catch (error) {
    box.hidden = true;
    announce(errorText(error));
  }
}

$("install-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const path = $("manifest-path").value.trim();
  if (path) showPreview(path);
});

// ---- the marketplace -----------------------------------------------------------

async function renderMarket(keepStatus = false) {
  if (!keepStatus) $("market-status").textContent = t("market.loading");
  try {
    const { apps, settings } = await invoke("catalogue");
    $("market-previews").checked = settings.channel === "preview";
    // Name applications that are not installed yet, for "Use <name>".
    for (const app of apps) if (!appNames[app.id]) appNames[app.id] = app.name;
    if (!keepStatus) $("market-status").textContent = apps.length ? "" : t("market.empty");
    $("market-list").replaceChildren(...apps.map(marketCard));
  } catch (error) {
    $("market-status").textContent = errorText(error);
  }
}

function marketCard(app) {
  const publisher = [app.publisher?.name || "", app.publisher?.verified ? `(${t("market.verified")})` : ""].join(" ").trim();
  const header = el("header", {},
    el("h2", { text: app.name }),
    app.version ? el("span", { class: "muted", text: t("market.version", { version: app.version, channel: app.channel }) }) : null,
    app.official ? el("span", { class: "state running", text: t("market.official") }) : null,
  );
  const body = [
    el("p", { text: app.summary }),
    el("p", { class: "muted", text: `${t("market.by", { publisher })} · ${app.license}` }),
  ];
  const actions = el("div", { class: "actions" });
  if (app.state === "unavailable") {
    body.push(el("p", { class: "muted", text: t("market.unavailable", { reason: app.reason }) }));
  } else {
    const caps = app.capabilities || [];
    body.push(caps.length
      ? el("div", {}, el("p", { text: t("apps.install.asks") }), el("ul", {}, ...caps.map((c) => el("li", { text: capabilityText(c) }))))
      : el("p", { class: "muted", text: t("apps.install.asks.nothing") }));
    if (!app.signed && app.state !== "installed") {
      body.push(el("p", { class: "warning", text: t("market.unsigned") }));
    }
    if (app.state === "installed") {
      actions.append(el("span", { class: "state running", text: t("market.installed") }));
    } else {
      const label = app.state === "update" ? t("market.update", { version: app.version }) : t("market.install");
      actions.append(el("button", {
        text: app.signed ? label : t("market.unsigned.confirm"),
        "aria-label": `${label}: ${app.name}`,
        onclick: () => installFromMarket(app, !app.signed),
      }));
    }
  }
  return el("li", { class: "card" }, header, ...body, actions);
}

async function installFromMarket(app, allowUnsigned) {
  $("market-status").textContent = t("market.installing", { name: app.name });
  try {
    await invoke("install_market", { app: app.id, allowUnsigned });
    announce(t("market.done", { name: app.name }));
    $("market-status").textContent = t("market.done", { name: app.name });
  } catch (error) {
    $("market-status").textContent = t("market.refused", { reason: (error && (error.message || error.code)) || String(error) });
  }
  // Keep the outcome on screen while the list refreshes under it.
  await renderMarket(true);
  await renderApps();
}

$("market-previews").addEventListener("change", async (event) => {
  try {
    await invoke("market_channel", { channel: event.target.checked ? "preview" : "stable" });
  } catch (error) {
    announce(errorText(error));
  }
  renderMarket();
});

// ---- search --------------------------------------------------------------------

let searchTimer = 0;
async function runSearch() {
  const query = $("search-query").value.trim();
  if (!query) {
    $("search-results").replaceChildren();
    $("search-status").textContent = "";
    return;
  }
  try {
    const { hits } = await invoke("search", { query });
    $("search-status").textContent = hits.length ? t("search.count", { count: hits.length }) : t("search.empty");
    $("search-results").replaceChildren(...hits.map((hit) => {
      const name = appNames[hit.app] || hit.app;
      return el("li", {},
        el("div", { class: "row" },
          el("div", {},
            el("h2", { text: hit.title || hit.id }),
            el("div", { class: "meta", text: name }),
          ),
          el("button", { class: "secondary", text: t("search.open"), "aria-label": `${t("search.open")}: ${hit.title || hit.id}`, onclick: async () => {
            try {
              const outcome = await invoke("open", { app: hit.app, id: hit.id });
              if (!outcome.opened) {
                announce(outcome.reason === "app_unavailable" ? t("search.unavailable", { name }) : t("search.cannot.open", { name }));
              }
            } catch (error) {
              announce(errorText(error));
            }
          } }),
        ),
        hit.snippet ? el("p", { class: "muted", text: hit.snippet }) : null,
      );
    }));
  } catch (error) {
    $("search-status").textContent = errorText(error);
  }
}
$("search-query").addEventListener("input", () => {
  window.clearTimeout(searchTimer);
  searchTimer = window.setTimeout(runSearch, 200);
});
$("search-form").addEventListener("submit", (event) => { event.preventDefault(); runSearch(); });

// ---- notifications --------------------------------------------------------------

async function renderNotifications() {
  try {
    const all = (await invoke("notifications", { unreadOnly: false })).items || [];
    const unread = all.filter((n) => !n.read);
    $("unread-badge").hidden = unread.length === 0;
    $("unread-badge").textContent = String(unread.length);
    const shown = $("unread-only").checked ? unread : all;
    $("notifications-empty").hidden = shown.length > 0;
    $("notification-list").replaceChildren(...shown.map((n) => el("li", { class: n.read ? "" : "unread" },
      el("div", { class: "row" },
        el("div", {},
          el("h2", { text: n.title }),
          el("div", { class: "meta", text: `${appNames[n.app] || n.app} · ${new Date(n.at).toLocaleString(document.documentElement.lang)}` }),
        ),
        n.read ? null : el("button", { class: "secondary", text: t("notifications.mark.read"), onclick: () => act(invoke("mark_read", { id: n.id }), renderNotifications) }),
      ),
      n.body ? el("p", { text: n.body }) : null,
    )));
  } catch (error) {
    announce(errorText(error));
  }
}
$("unread-only").addEventListener("change", renderNotifications);

// ---- activity --------------------------------------------------------------------

async function renderActivity() {
  try {
    const { entries, verify } = await invoke("activity");
    showVerify(verify);
    $("activity-list").replaceChildren(...entries.map((e) => el("li", {},
      el("div", { class: "meta", text: `${new Date(e.at).toLocaleString(document.documentElement.lang)} · ${e.actor}` }),
      el("div", { text: `${e.action} — ${e.target}` }),
    )));
  } catch (error) {
    announce(errorText(error));
  }
}
function showVerify(verify) {
  $("verify-status").textContent = verify.ok
    ? t("activity.verified", { count: verify.entries })
    : t("activity.broken", { at: verify.broken_at });
}
$("verify").addEventListener("click", renderActivity);

// ---- preferences controls ----------------------------------------------------------

$("pref-locale").addEventListener("change", (e) => setPref("locale", e.target.value));
$("pref-theme").addEventListener("change", (e) => setPref("theme", e.target.value));
$("pref-reduce-motion").addEventListener("change", (e) => setPref("reduce_motion", e.target.checked));
$("pref-high-contrast").addEventListener("change", (e) => setPref("high_contrast", e.target.checked));
$("pref-font-scale").addEventListener("change", (e) => setPref("font_scale", Number(e.target.value)));

// ---- live events from the core --------------------------------------------------------

listen("core-event", async ({ payload }) => {
  const topic = payload.topic || "";
  if (topic === "core/prefs.changed") {
    const before = prefs.locale;
    applyPrefs(await invoke("prefs"));
    if (prefs.locale !== before) {
      await loadStrings(prefs.locale);
      showCore(coreState.key, coreState.vars, coreState.error);
      refresh(currentTab().id);
    }
  } else if (topic === "core/notification.posted") {
    const n = payload.payload || {};
    announce(t("notifications.new", { name: appNames[n.app] || n.app || "", title: n.title || "" }));
    renderNotifications();
  } else if (topic.startsWith("core/app.")) {
    if (currentTab().id === "tab-apps") renderApps();
  } else if (topic === "core/shutdown") {
    showCore("core.failed", { reason: "shutdown" }, true);
  }
});

listen("manifest-dropped", ({ payload }) => {
  selectTab($("tab-apps"));
  $("manifest-path").value = payload;
  showPreview(payload);
});

// ---- start ---------------------------------------------------------------------------

async function start() {
  await loadStrings("system");
  showCore("core.connecting");
  try {
    const status = await invoke("status");
    applyPrefs(await invoke("prefs"));
    await loadStrings(prefs.locale);
    showCore("core.connected", { version: status.core.core_version });
    await renderApps();
    await renderNotifications();
  } catch (error) {
    showCore("core.failed", { reason: (error && (error.message || error.code)) || String(error) }, true);
  }
}
$("retry").addEventListener("click", start);
start();
