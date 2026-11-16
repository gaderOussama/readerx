const { invoke, convertFileSrc } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

const $ = (id) => document.getElementById(id);

const SAVE_DELAY_MS = 1000;
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
  win.document.addEventListener("keydown", onViewerKey);
  attach(session, win);
});

async function attach(session, win) {
  const app = win.PDFViewerApplication;
  await app.initializedPromise;
  if (cur !== session || session.frame.contentWindow !== win) return;
  session.app = app;
  app.eventBus.on("updateviewarea", (e) => onViewArea(session, app, e.location));
  try {
    await app.open({ url: convertFileSrc(session.path) });
  } catch (err) {
    // PDF.js shows its own error message; Back keeps working.
    console.error("viewer open failed", err);
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

// Saves run one after another so an older position never lands after a newer one.
function flush(session) {
  clearTimeout(session.timer);
  session.queue = session.queue.then(() => send(session));
  return session.queue;
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

export function openReader({ book, path, position, theme, onExit }) {
  cur = { id: book.id, path, position, theme, onExit, queue: Promise.resolve(), exiting: false };
  $("reader-title").textContent = book.displayTitle;
  $("reader-view").hidden = false;
  loadViewer(cur);
}

$("reader-back").addEventListener("click", back);

getCurrentWindow()
  .onCloseRequested(async () => {
    if (cur) await flush(cur);
  })
  .catch((err) => console.error("close handler not set", err));
