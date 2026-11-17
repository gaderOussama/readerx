import { initHighlights, startHighlights, attachHighlights, highlightKey, flushNotes } from "./highlights.js";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { openUrl } = window.__TAURI__.opener;

const $ = (id) => document.getElementById(id);

const SAVE_DELAY_MS = 1000;
const PALETTE = ["bg", "surface", "text", "muted", "line", "highlight", "highlight-blend"];
const VIEWER = "vendor/pdfjs/web/viewer.html";

// The one open book, or null. Every async step re-checks that it is still the
// same session, because Back or a theme switch can replace it mid-flight.
let cur = null;

function cssColor(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim();
}

function viewerUrl(position) {
  if (!position) return VIEWER;
  const { page, zoom, left, top } = position;
  return `${VIEWER}#page=${page}&zoom=${zoom},${Math.round(left)},${Math.round(top)}`;
}

function loadViewer(session) {
  clearTimeout(session.timer);
  const start = session.pending ?? session.position;
  session.app = null;
  session.pending = null;
  const frame = document.createElement("iframe");
  frame.title = "PDF viewer";
  session.frame = frame;
  $("viewer-host").replaceChildren(frame);
  frame.src = viewerUrl(start);
}

// Runs inside the viewer's own `webviewerloaded` event, before it reads its options.
document.addEventListener("webviewerloaded", (e) => {
  const win = e.detail.source;
  if (!cur || win !== cur.frame.contentWindow) return;
  const session = cur;
  const { theme } = session;
  const options = win.PDFViewerApplicationOptions;
  options.set("defaultUrl", "");
  options.set("disablePreferences", true);
  options.set("externalLinkTarget", 2);
  options.set("viewerCssTheme", theme === "night" ? 2 : 1);
  if (theme !== "original") {
    options.set("forcePageColors", true);
    options.set("pageColorsBackground", cssColor("bg"));
    options.set("pageColorsForeground", cssColor("text"));
  }
  themeViewer(win);
  lockDown(win);
  attach(session, win);
});

function openExternal(href) {
  let url;
  try {
    url = new URL(href);
  } catch {
    return;
  }
  if (["http:", "https:", "mailto:"].includes(url.protocol)) {
    openUrl(url.href).catch((err) => console.error("could not open link", err));
  }
}

function themeViewer(win) {
  const doc = win.document;
  for (const name of PALETTE) doc.documentElement.style.setProperty(`--rx-${name}`, cssColor(name));
  const sheet = doc.createElement("link");
  sheet.rel = "stylesheet";
  sheet.href = new URL("viewer-theme.css", document.baseURI).href;
  doc.head.append(sheet);
}

// The viewer shows one book and nothing else: no other files in, no print or
// save out, and links leave through the system browser.
function lockDown(win) {
  const doc = win.document;
  for (const type of ["dragover", "drop"]) {
    doc.addEventListener(type, (e) => {
      e.preventDefault();
      e.stopPropagation();
    }, true);
  }
  // A target="_blank" link never calls window.open, so catch the click too.
  // Links inside the document are "#..." and stay with the viewer.
  doc.addEventListener("click", (e) => {
    const link = e.target.closest?.("a[href]");
    if (!link || link.getAttribute("href").startsWith("#")) return;
    e.preventDefault();
    e.stopPropagation();
    openExternal(link.href);
  }, true);
  win.open = (url) => {
    openExternal(url);
    return null;
  };
  doc.addEventListener("keydown", onViewerKey, true);
}

async function attach(session, win) {
  const app = win.PDFViewerApplication;
  await app.initializedPromise;
  if (cur !== session || session.frame.contentWindow !== win) return;
  session.app = app;
  attachHighlights(session, win, app);
  app.eventBus.on("updateviewarea", (e) => onViewArea(session, app, e.location));
  try {
    await app.open({ url: convertFileSrc(session.path) });
  } catch (err) {
    // This PDF.js build only logs load errors, so say it here. Back keeps working.
    console.error("viewer open failed", err);
    if (cur === session && session.frame.contentWindow === win) {
      const note = document.createElement("p");
      note.className = "viewer-error";
      note.textContent = "This PDF can't be opened. The file may be damaged.";
      $("viewer-host").append(note);
    }
  }
}

function onViewArea(session, app, location) {
  // Until the viewer has jumped to the saved spot, its position is not the reader's.
  if (cur !== session || session.app !== app || app.isInitialViewSet !== true) return;
  const box = app.pdfViewer.container;
  session.pending = {
    kind: "pdf",
    page: location.pageNumber,
    top: location.top,
    left: location.left,
    zoom: location.scale,
    atEnd: box.scrollTop + box.clientHeight >= box.scrollHeight - 2,
  };
  clearTimeout(session.timer);
  session.timer = setTimeout(() => flush(session), SAVE_DELAY_MS);
}

// Saves and highlight calls run one after another, so an older one never lands
// after a newer one. A failed task does not stop the queue.
function enqueue(session, task) {
  const run = session.queue.then(task);
  session.queue = run.catch(() => {});
  return run;
}

function flush(session) {
  clearTimeout(session.timer);
  flushNotes(session);
  return enqueue(session, () => send(session));
}

async function send(session) {
  const { app, pending } = session;
  if (!pending || app?.isInitialViewSet !== true) return;
  session.pending = null;
  session.position = pending;
  try {
    await invoke("save_position", { id: session.id, position: pending, pageCount: app.pagesCount });
  } catch (err) {
    console.error("save_position failed", err);
    session.pending ??= pending;
  }
}

function viewerIsBusy(doc) {
  const open = (id) => !doc.getElementById(id)?.classList.contains("hidden");
  return open("findbar") || open("secondaryToolbar") || doc.querySelector("dialog[open]") || doc.fullscreenElement;
}

function onViewerKey(e) {
  // Open, print and save shortcuts would undo the lockdown.
  if ((e.ctrlKey || e.metaKey) && ["o", "p", "s"].includes(e.key.toLowerCase())) {
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  if (highlightKey(e, cur)) return;
  if (e.key !== "Escape" || viewerIsBusy(e.currentTarget)) return;
  e.preventDefault();
  back();
}

async function back() {
  const session = cur;
  if (!session || session.exiting) return;
  session.exiting = true;
  await flush(session);
  cur = null;
  $("viewer-host").replaceChildren();
  $("reader-view").hidden = true;
  await session.onExit();
}

function markTheme(theme) {
  for (const button of document.querySelectorAll("[data-theme-choice]")) {
    button.setAttribute("aria-pressed", String(button.dataset.themeChoice === theme));
  }
}

// The viewer reads page colors only at start, so a switch reloads it at the saved spot.
async function switchTheme(theme) {
  const session = cur;
  if (!session || session.exiting || session.switching || theme === session.theme) return;
  session.switching = true;
  try {
    await flush(session);
    await invoke("set_page_theme", { theme });
    if (cur !== session) return;
    session.theme = theme;
    session.onTheme(theme);
    markTheme(theme);
    loadViewer(session);
  } catch (err) {
    console.error("theme switch failed", err);
  } finally {
    session.switching = false;
  }
}

export function openReader({ book, path, position, theme, onExit, onTheme }) {
  cur = { id: book.id, path, position, theme, onExit, onTheme, queue: Promise.resolve(), exiting: false };
  startHighlights(cur);
  $("reader-title").textContent = book.displayTitle;
  markTheme(theme);
  $("reader-view").hidden = false;
  loadViewer(cur);
}

initHighlights({ current: () => cur, enqueue, back });

$("reader-back").addEventListener("click", back);
for (const button of document.querySelectorAll("[data-theme-choice]")) {
  button.addEventListener("click", () => switchTheme(button.dataset.themeChoice));
}

getCurrentWindow()
  .onCloseRequested(async () => {
    if (cur) await flush(cur);
  })
  .catch((err) => console.error("close handler not set", err));
