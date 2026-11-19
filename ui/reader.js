import { initHighlights, startHighlights, attachHighlights, highlightKey, flushNotes, endHighlights, closePanelIfOpen } from "./highlights.js";
import { closeToc, tocIsOpen } from "./toc.js";
import { columnCount, stepFontSize, themeStyles, isExternalHref, blockScripts } from "./lib/epub.js";
import { isProtected } from "./lib/epub-meta.js";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;
const { openUrl } = window.__TAURI__.opener;

const $ = (id) => document.getElementById(id);

const SAVE_DELAY_MS = 1000;
const PALETTE = ["bg", "surface", "text", "muted", "line", "highlight", "highlight-blend"];
const VIEWER = "vendor/pdfjs/web/viewer.html";
const EPUB_ERROR = "This book can't be opened. It may be damaged or copy protected.";

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
  if (session.hl) session.hl.v = null;
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
  const epub = session.format === "epub";
  if (!pending || !(epub ? session.ready : app?.isInitialViewSet === true)) return;
  session.pending = null;
  // For EPUB `atEnd` travels beside the position and there is no page count.
  const { atEnd, ...position } = pending;
  session.position = epub ? position : pending;
  try {
    await invoke("save_position", epub
      ? { id: session.id, position, atEnd }
      : { id: session.id, position: pending, pageCount: app.pagesCount });
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
  if (closePanelIfOpen(cur)) return;
  back();
}

async function back() {
  const session = cur;
  if (!session || session.exiting) return;
  session.exiting = true;
  await flush(session);
  if (session.format === "epub") closeEpub(session);
  else endHighlights(session);
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

// The PDF viewer reads page colors only at start, so a switch reloads it at the saved spot.
// An EPUB just gets its styles again.
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
    if (session.format === "epub") applyStyles(session);
    else loadViewer(session);
  } catch (err) {
    console.error("theme switch failed", err);
  } finally {
    session.switching = false;
  }
}

// ---- EPUB ----

// For the Contents panel: the open EPUB view, or null.
export function currentEpubView() {
  return cur?.format === "epub" ? cur.view ?? null : null;
}

const isTyping = (e) => /^(INPUT|TEXTAREA)$/.test(e.target?.tagName ?? "");
const stopDefault = (e) => e.preventDefault();

// A mouse click leaves the bar button focused, and Space would press it again
// instead of turning the page.
function dropFocus(e) {
  if (e.detail && cur?.format === "epub") e.currentTarget.blur();
}

function markLayout(layout) {
  for (const button of document.querySelectorAll("[data-layout-choice]")) {
    button.setAttribute("aria-pressed", String(button.dataset.layoutChoice === layout));
  }
}

async function openEpub(session) {
  const host = $("viewer-host");
  document.addEventListener("keydown", onEpubKey, true);
  for (const type of ["contextmenu", "dragover", "drop"]) document.addEventListener(type, stopDefault);
  let view;
  try {
    await import("./vendor/foliate-js/view.js");
    if (cur !== session) return;
    view = document.createElement("foliate-view");
    session.view = view;
    view.addEventListener("relocate", (e) => onRelocate(session, e.detail));
    view.addEventListener("load", (e) => hookDoc(session, e.detail.doc));
    // foliate-js would call window.open with any href, javascript: included.
    view.addEventListener("external-link", (e) => {
      e.preventDefault();
      if (isExternalHref(e.detail.href_)) openExternal(e.detail.href_);
    });
    host.replaceChildren(view);
    const res = await fetch(convertFileSrc(session.path));
    if (!res.ok) throw new Error(`could not read the file (${res.status})`);
    const file = new File([await res.blob()], session.fileName);
    if (cur !== session) return;
    await view.open(file);
    if (cur !== session) {
      view.book?.destroy?.();
      return;
    }
    const { book } = view;
    // Before any page loads: book pages don't get the app's CSP (see blockScripts).
    book.transformTarget?.addEventListener("load", (e) => {
      if (e.detail.isScript) e.detail.allow = false;
    });
    book.transformTarget?.addEventListener("data", (e) => {
      if (/html|svg/.test(e.detail.type)) e.detail.data = Promise.resolve(e.detail.data).then(blockScripts);
    });
    const [rights, encryption] = await Promise.all([
      book.loadText("META-INF/rights.xml"),
      book.loadText("META-INF/encryption.xml"),
    ]);
    if (isProtected(rights, encryption)) throw new Error("copy protected");
    if (cur !== session) return;

    // The renderer that was chosen, which also covers display-options fixed layout.
    session.fixed = view.isFixedLayout;
    $("epub-flow").hidden = session.fixed;
    applyStyles(session);
    if (!session.fixed) {
      view.renderer.setAttribute("flow", session.epub.layout === "scrolled" ? "scrolled" : "paginated");
      applyColumns(session);
    }
    await resumeEpub(session, view);
    if (cur !== session) return;
    session.ready = true;
    session.observer = new ResizeObserver(() => applyColumns(session));
    session.observer.observe(host);
  } catch (err) {
    console.error("epub open failed", err);
    if (cur !== session) return;
    dropView(session);
    $("epub-flow").hidden = true;
    $("toc-toggle").hidden = true;
    const note = document.createElement("p");
    note.className = "viewer-error";
    note.textContent = EPUB_ERROR;
    host.replaceChildren(note);
  }
}

// A CFI that doesn't resolve does not reliably throw in foliate-js (it opens the
// first section or shows nothing), so check first and fall back to the fraction.
async function resumeEpub(session, view) {
  const pos = session.position?.kind === "epub" ? session.position : null;
  if (!pos) return view.init({});
  let found = null;
  try {
    found = view.resolveNavigation(pos.cfi);
  } catch {}
  if (found && found.index >= 0) {
    try {
      await view.init({ lastLocation: pos.cfi });
    } catch (err) {
      console.error("resume by cfi failed", err);
    }
  }
  if (cur === session && !view.lastLocation) await view.goToFraction(pos.fraction);
}

// The section documents can't see the app's CSS variables, so the palette goes in as literal colors.
function applyStyles(session) {
  const { theme, epub, view } = session;
  $("viewer-host").style.background = theme === "original" ? "#fff" : cssColor("bg");
  if (session.fixed) return;
  const colors = { text: cssColor("text"), bg: cssColor("bg") };
  view.renderer.setStyles(themeStyles(theme, colors, epub.fontSize));
}

// max-column-count alone does not lay out again, so render() follows it.
function applyColumns(session) {
  const renderer = session.view?.renderer;
  if (session.fixed || !renderer) return;
  const count = columnCount($("viewer-host").clientWidth);
  if (count === session.columns) return;
  session.columns = count;
  renderer.setAttribute("max-column-count", String(count));
  renderer.render();
}

function onRelocate(session, detail) {
  if (cur !== session || !session.ready || session.exiting) return;
  const { cfi, fraction } = detail;
  if (!cfi || !Number.isFinite(fraction)) return;
  session.pending = {
    kind: "epub",
    cfi,
    fraction: Math.min(1, Math.max(0, fraction)),
    chapter: detail.tocItem?.label?.trim().slice(0, 200) || null,
    atEnd: session.view.renderer.atEnd === true,
  };
  clearTimeout(session.timer);
  session.timer = setTimeout(() => flush(session), SAVE_DELAY_MS);
}

// Size and layout: remember the spot, apply, save the setting, then go back to the spot.
function changeEpub(patch) {
  const session = cur;
  if (session?.format !== "epub" || !session.ready || session.fixed || session.exiting) return;
  const next = { ...session.epub, ...patch };
  if (next.fontSize === session.epub.fontSize && next.layout === session.epub.layout) return;
  const cfi = session.view.lastLocation?.cfi;
  const layoutChanged = next.layout !== session.epub.layout;
  session.epub = next;
  markLayout(next.layout);
  applyStyles(session);
  if (layoutChanged) session.view.renderer.setAttribute("flow", next.layout === "scrolled" ? "scrolled" : "paginated");
  invoke("set_epub_settings", { fontSize: next.fontSize, layout: next.layout })
    .then(() => session.onEpubSettings?.(next.fontSize, next.layout))
    .catch((err) => console.error("set_epub_settings failed", err));
  if (cfi) session.view.goTo(cfi);
}

// Keys on the reader page and, in the capture phase, on every section document.
function onEpubKey(e) {
  const session = cur;
  if (session?.format !== "epub" || session.exiting) return;
  const { key } = e;
  const mod = e.ctrlKey || e.metaKey;
  const letter = key.toLowerCase();
  // Reload, devtools, open, print and save would undo the lockdown (also in the search field).
  if (key === "F5" || key === "F12" || (mod && (["r", "o", "p", "s"].includes(letter) || (e.shiftKey && ["i", "j", "c"].includes(letter))))) {
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  if (isTyping(e) && key !== "Escape") return;
  if (key === "Escape") {
    e.preventDefault();
    if (tocIsOpen()) closeToc();
    else back();
    return;
  }
  const view = session.view;
  if (!session.ready || !view) return;
  if (mod && !e.altKey) {
    const smaller = key === "-" || e.code === "NumpadSubtract";
    if (smaller || key === "=" || key === "+" || e.code === "NumpadAdd") {
      e.preventDefault();
      changeEpub({ fontSize: stepFontSize(session.epub.fontSize, smaller ? -1 : 1) });
    }
    return;
  }
  if (e.altKey || (key === " " && e.target?.tagName === "BUTTON")) return;
  const turn = { ArrowLeft: () => view.goLeft(), ArrowRight: () => view.goRight(), PageDown: () => view.next(), PageUp: () => view.prev() };
  turn[" "] = e.shiftKey ? turn.PageUp : turn.PageDown;
  if (!turn[key]) return;
  e.preventDefault();
  turn[key]();
}

// Each section document: keys, lockdown, edge clicks, window.open.
function hookDoc(session, doc) {
  doc.addEventListener("keydown", onEpubKey, true);
  for (const type of ["contextmenu", "dragover", "drop"]) doc.addEventListener(type, stopDefault);
  doc.addEventListener("click", (e) => onEdgeClick(session, doc, e));
  doc.defaultView.open = (url) => {
    if (isExternalHref(url)) openExternal(url);
    return null;
  };
}

// Pages mode only (and fixed layout, which is page by page): the outer fifths of
// the reader turn the page, unless the click is on a link or ends a selection.
function onEdgeClick(session, doc, e) {
  if (cur !== session || session.exiting || !session.ready) return;
  if (!session.fixed && session.epub.layout !== "paginated") return;
  if (e.button !== 0 || e.target.closest?.("a") || !doc.getSelection().isCollapsed) return;
  // The paginator's frame is as wide as all columns and scrolled, and fixed layout
  // frames are scaled with CSS, so the frame rectangle gives the screen position.
  const frame = doc.defaultView.frameElement;
  const rect = frame.getBoundingClientRect();
  const x = rect.left + e.clientX * (rect.width / (frame.offsetWidth || rect.width));
  const box = session.view.getBoundingClientRect();
  if (x < box.left + box.width / 5) session.view.goLeft();
  else if (x > box.right - box.width / 5) session.view.goRight();
}

// close() throws when no section ever loaded; the book's blob URLs need destroy().
function dropView(session) {
  const { view } = session;
  if (!view) return;
  session.view = null;
  session.observer?.disconnect();
  try {
    view.close();
  } catch (err) {
    console.error("epub close failed", err);
  }
  view.book?.destroy?.();
  view.remove();
}

function closeEpub(session) {
  clearTimeout(session.timer);
  closeToc();
  dropView(session);
  document.removeEventListener("keydown", onEpubKey, true);
  for (const type of ["contextmenu", "dragover", "drop"]) document.removeEventListener(type, stopDefault);
  $("viewer-host").style.background = "";
}

export function openReader({ book, path, position, theme, onExit, onTheme, epub, onEpubSettings }) {
  const isEpub = book.format === "epub";
  cur = { id: book.id, path, position, theme, onExit, onTheme, queue: Promise.resolve(), exiting: false, format: isEpub ? "epub" : "pdf" };
  $("hl-toggle").hidden = isEpub;
  $("toc-toggle").hidden = !isEpub;
  $("epub-flow").hidden = !isEpub;
  $("reader-title").textContent = book.displayTitle;
  markTheme(theme);
  $("reader-view").hidden = false;
  if (isEpub) {
    Object.assign(cur, { fileName: book.fileName, epub: { fontSize: 100, layout: "paginated", ...epub }, onEpubSettings, ready: false });
    markLayout(cur.epub.layout);
    openEpub(cur);
  } else {
    startHighlights(cur);
    loadViewer(cur);
  }
}

initHighlights({ current: () => cur, enqueue });

$("reader-back").addEventListener("click", back);
for (const button of document.querySelectorAll("[data-theme-choice]")) {
  button.addEventListener("click", (e) => {
    dropFocus(e);
    switchTheme(button.dataset.themeChoice);
  });
}

getCurrentWindow()
  .onCloseRequested(async () => {
    if (cur) await flush(cur);
  })
  .catch((err) => console.error("close handler not set", err));

$("epub-smaller").addEventListener("click", (e) => {
  dropFocus(e);
  changeEpub({ fontSize: stepFontSize(cur?.epub?.fontSize, -1) });
});
$("epub-bigger").addEventListener("click", (e) => {
  dropFocus(e);
  changeEpub({ fontSize: stepFontSize(cur?.epub?.fontSize, 1) });
});
for (const button of document.querySelectorAll("[data-layout-choice]")) {
  button.addEventListener("click", (e) => {
    dropFocus(e);
    changeEpub({ layout: button.dataset.layoutChoice });
  });
}
