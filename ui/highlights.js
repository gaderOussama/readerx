import { h } from "./lib/dom.js";
import {
  LIMITS, cleanText, normalizeRect, rectsIntersect, pointInRect, mergeLineRects, percentBox, sortHighlights,
  middleTruncate, errorWords,
} from "./lib/highlights.js";

const { invoke } = window.__TAURI__.core;
const { open: openDialog } = window.__TAURI__.dialog;

const $ = (id) => document.getElementById(id);

const TOO_LONG = "That highlight is too long.";
const NOT_SAVED = "Could not save that highlight.";
const NOTE_NOT_SAVED = "Could not save that note.";
const NOTE_MS = 1000;
const PATH_MAX = 28;
const BAR_MS = 4000;
const UNDO_MS = 6000;

// reader.js hands over how to read the open session and how to queue a call.
let ctx;
export function initHighlights(context) {
  ctx = context;
}

const live = (session) => ctx.current() === session && !session.exiting;

// Every highlight call runs on the reader session's ordered queue.
const call = (session, cmd, args = {}) => ctx.enqueue(session, () => invoke(cmd, { id: session.id, ...args }));

// ---- bar (messages and Undo), in the parent page ----

let barTimer = 0;
let barAction = null;

export function hideBar() {
  clearTimeout(barTimer);
  barAction = null;
  $("hl-bar").hidden = true;
}

function showBar(text, action = null) {
  clearTimeout(barTimer);
  barAction = action;
  $("hl-bar-text").textContent = text;
  $("hl-undo").hidden = action == null;
  $("hl-bar").hidden = false;
  barTimer = setTimeout(hideBar, action ? UNDO_MS : BAR_MS);
}

$("hl-undo").addEventListener("click", () => {
  const action = barAction;
  hideBar();
  action?.();
});

// ---- session state ----

export function startHighlights(session) {
  session.hl = { list: [], v: null, open: false, dirty: new Map(), timers: new Map() };
  $("hl-toggle").textContent = "Highlights (0)";
  fetchHighlights(session);
}

function setList(session, list) {
  session.hl.list = sortHighlights(list);
  $("hl-toggle").textContent = `Highlights (${session.hl.list.length})`;
  redraw(session);
  renderPanel(session);
}

async function fetchHighlights(session) {
  try {
    const res = await call(session, "get_highlights");
    if (!live(session)) return;
    Object.assign(session.hl, { exportFolder: res.exportFolder, exportError: res.exportError, notice: res.notice });
    setList(session, res.highlights);
  } catch (err) {
    console.error("get_highlights failed", err);
  }
}

// ---- viewer side: selection, button, drawing ----

export function attachHighlights(session, win, app) {
  const doc = win.document;
  const button = doc.createElement("button");
  button.type = "button";
  button.className = "rx-hl-btn";
  button.textContent = "Highlight";
  button.hidden = true;
  button.addEventListener("mousedown", (e) => e.preventDefault());
  button.addEventListener("click", () => addFromSelection(session));
  doc.body.append(button);
  const v = { win, doc, app, button, menu: null, down: false, timer: 0 };
  session.hl.v = v;

  doc.addEventListener("mousedown", (e) => {
    v.down = true;
    hideButton(v);
    if (v.menu && !v.menu.contains(e.target)) closeMenu(v);
  }, true);
  doc.addEventListener("click", (e) => onPageClick(session, v, e));
  doc.addEventListener("mouseup", () => {
    v.down = false;
    showButton(v);
  }, true);
  doc.addEventListener("selectionchange", () => {
    hideButton(v);
    clearTimeout(v.timer);
    if (!v.down) v.timer = setTimeout(() => showButton(v), 200);
  });
  const quiet = () => {
    hideButton(v);
    closeMenu(v);
  };
  app.pdfViewer.container.addEventListener("scroll", quiet, { passive: true });
  app.eventBus.on("scalechanging", quiet);
  const place = (e) => live(session) && session.hl.v === v && drawPage(session, e.source);
  app.eventBus.on("pagerendered", place);
  app.eventBus.on("textlayerrendered", place);
  redraw(session);
}

function hideButton(v) {
  clearTimeout(v.timer);
  v.button.hidden = true;
}

function* textNodes(doc, range) {
  const root = range.commonAncestorContainer;
  const walker = doc.createTreeWalker(root.nodeType === 3 ? root.parentNode : root, 4);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) if (range.intersectsNode(n)) yield n;
}

// The selection, but only when all of its text sits in page text layers.
function readSelection(v) {
  const sel = v.win.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) return null;
  const range = sel.getRangeAt(0);
  let any = false;
  for (const n of textNodes(v.doc, range)) {
    if (!n.data.trim()) continue;
    if (!n.parentElement?.closest(".textLayer")) return null;
    any = true;
  }
  return any ? range : null;
}

function showButton(v) {
  const range = readSelection(v);
  if (!range) return hideButton(v);
  let last = null;
  for (const n of textNodes(v.doc, range)) {
    const part = v.doc.createRange();
    part.selectNodeContents(n);
    if (n === range.startContainer) part.setStart(n, range.startOffset);
    if (n === range.endContainer) part.setEnd(n, range.endOffset);
    const rects = part.getClientRects();
    if (rects.length) last = rects[rects.length - 1];
  }
  if (!last) return hideButton(v);
  const { button } = v;
  button.hidden = false;
  const room = v.win.innerWidth - button.offsetWidth - 8;
  button.style.left = `${Math.max(8, Math.min(last.right, room))}px`;
  button.style.top = `${Math.min(last.bottom + 6, v.win.innerHeight - button.offsetHeight - 8)}px`;
}

// The range cut down to one page's text layer.
function clipTo(doc, range, layer) {
  const box = doc.createRange();
  box.selectNodeContents(layer);
  const out = range.cloneRange();
  if (out.compareBoundaryPoints(out.START_TO_START, box) < 0) out.setStart(box.startContainer, box.startOffset);
  if (out.compareBoundaryPoints(out.END_TO_END, box) > 0) out.setEnd(box.endContainer, box.endOffset);
  return out;
}

// Client boxes of the text nodes only (no element boxes) and the cleaned text.
function collect(doc, range, layer) {
  const boxes = [];
  let text = "";
  const walker = doc.createTreeWalker(layer, 5);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (!range.intersectsNode(n)) continue;
    if (n.nodeType === 1) {
      if (n.tagName === "BR") text += "\n";
      continue;
    }
    const part = doc.createRange();
    part.selectNodeContents(n);
    if (n === range.startContainer) part.setStart(n, range.startOffset);
    if (n === range.endContainer) part.setEnd(n, range.endOffset);
    text += part.toString();
    for (const r of part.getClientRects()) boxes.push([r.left, r.top, r.right, r.bottom]);
  }
  return { boxes: mergeLineRects(boxes), text: cleanText(text) };
}

// Client pixels on a page's text layer <-> PDF units.
function pageMap(pageView, layer) {
  const b = layer.getBoundingClientRect();
  const vp = pageView.viewport;
  const sx = b.width ? vp.width / b.width : 1;
  const sy = b.height ? vp.height / b.height : 1;
  return {
    vp, b, sx, sy,
    toPdf: (x, y) => vp.convertToPdfPoint((x - b.left) * sx, (y - b.top) * sy),
  };
}

const round = (n) => Math.round(n * 1000) / 1000;

function pdfRects(map, boxes) {
  return boxes.map(([l, t, r, bottom]) => {
    const [ax, ay] = map.toPdf(l, t);
    const [bx, by] = map.toPdf(r, bottom);
    return normalizeRect([ax, ay, bx, by]).map(round);
  });
}

function pagesOf(v, range) {
  const out = [];
  for (const page of v.doc.querySelectorAll(".page[data-page-number]")) {
    const layer = page.querySelector(":scope > .textLayer");
    if (!layer || !range.intersectsNode(page)) continue;
    const num = Number(page.dataset.pageNumber);
    const pageView = v.app.pdfViewer.getPageView(num - 1);
    const clipped = clipTo(v.doc, range, layer);
    const { boxes, text } = collect(v.doc, clipped, layer);
    if (!pageView || boxes.length === 0 || text === "") continue;
    const map = pageMap(pageView, layer);
    out.push({ page: num, rects: pdfRects(map, boxes), text, clipped, layer, map });
  }
  return out;
}

// When the new boxes overlap existing highlights, Rust merges them into one, so
// the quote must be the whole passage: the selection stretched over the old
// boxes. If a box can't be reached (scrolled away) the plain text is sent and
// Rust joins the quotes itself.
function quoteFor(session, v, item) {
  const hits = session.hl.list.filter((h) => h.page === item.page
    && h.rects.some((r) => item.rects.some((n) => rectsIntersect(r, n))));
  if (hits.length === 0) return item.text;
  const { vp, b, sx, sy } = item.map;
  const wide = item.clipped.cloneRange();
  for (const h of hits) {
    for (const rect of h.rects) {
      const [x1, y1, x2, y2] = normalizeRect(vp.convertToViewportRectangle(rect));
      const left = b.left + x1 / sx, right = b.left + x2 / sx;
      const top = b.top + y1 / sy, bottom = b.top + y2 / sy;
      const midX = (left + right) / 2, midY = (top + bottom) / 2;
      for (const [x, y] of [[left + 1, midY], [right - 1, midY], [midX, top + 1], [midX, bottom - 1]]) {
        const at = v.doc.caretRangeFromPoint(x, y);
        if (!at) return item.text;
        const [node, offset] = [at.startContainer, at.startOffset];
        if (wide.comparePoint(node, offset) < 0) wide.setStart(node, offset);
        else if (wide.comparePoint(node, offset) > 0) wide.setEnd(node, offset);
      }
    }
  }
  const { text } = collect(v.doc, clipTo(v.doc, wide, item.layer), item.layer);
  return text || item.text;
}

async function addFromSelection(session) {
  const v = session.hl.v;
  if (!v || !live(session)) return;
  const range = readSelection(v);
  if (!range) return;
  const items = pagesOf(v, range);
  for (const item of items) item.text = quoteFor(session, v, item);
  v.win.getSelection().removeAllRanges();
  hideButton(v);
  if (items.length === 0) return;
  if (items.some((i) => i.rects.length > LIMITS.rects || i.text.length > LIMITS.text)) return showBar(TOO_LONG);
  try {
    const res = await call(session, "add_highlights", {
      items: items.map(({ page, rects, text }) => ({ page, rects, text })),
    });
    if (live(session)) setList(session, res.highlights);
  } catch (code) {
    console.error("add_highlights failed", code);
    if (live(session)) showBar(code === "too_long" ? TOO_LONG : NOT_SAVED);
  }
}

function drawPage(session, pageView) {
  const v = session.hl.v;
  const div = pageView.div;
  const items = session.hl.list.filter((h) => h.page === pageView.id);
  let layer = div.querySelector(":scope > .rx-hl-layer");
  const wrapper = div.querySelector(":scope > .canvasWrapper");
  if (items.length === 0 || !wrapper) {
    layer?.remove();
    return;
  }
  if (!layer) {
    layer = v.doc.createElement("div");
    layer.className = "rx-hl-layer";
  }
  // Between the canvas and the text layer, whichever of them came first.
  const text = pageView.textLayer?.div;
  if (text?.parentNode === div) text.before(layer);
  else wrapper.after(layer);
  const vp = pageView.viewport;
  layer.replaceChildren(...items.flatMap((h) => h.rects.map((r) => {
    const box = percentBox(vp.convertToViewportRectangle(r), vp.width, vp.height);
    const el = v.doc.createElement("div");
    el.className = "rx-hl";
    el.style.left = `${box.left}%`;
    el.style.top = `${box.top}%`;
    el.style.width = `${box.width}%`;
    el.style.height = `${box.height}%`;
    return el;
  })));
}

function redraw(session) {
  const v = session.hl.v;
  if (!v) return;
  const pages = new Set(session.hl.list.map((h) => h.page));
  for (const layer of v.doc.querySelectorAll(".rx-hl-layer")) pages.add(Number(layer.parentNode.dataset.pageNumber));
  for (const n of pages) {
    const pageView = v.app.pdfViewer.getPageView(n - 1);
    if (pageView) drawPage(session, pageView);
  }
}

// ---- side panel, in the parent page ----

function openPanel(session) {
  session.hl.open = true;
  $("hl-panel").hidden = false;
  $("hl-toggle").setAttribute("aria-expanded", "true");
  renderPanel(session);
}

function closePanel(session) {
  flushNotes(session);
  session.hl.open = false;
  session.hl.notice = null;
  $("hl-panel").hidden = true;
  $("hl-toggle").setAttribute("aria-expanded", "false");
}

$("hl-toggle").addEventListener("click", () => {
  const session = ctx.current();
  if (!session || session.exiting) return;
  if (session.hl.open) closePanel(session);
  else openPanel(session);
});

function jump(session, hl) {
  const [x1, , , y2] = hl.rects[0];
  // 36 PDF units of room above the box, so it sits a little below the top.
  session.app?.pdfViewer.scrollPageIntoView({
    pageNumber: hl.page,
    destArray: [null, { name: "XYZ" }, x1, y2 + 36, null],
  });
}

function item(session, hl) {
  const note = h("textarea", {
    class: "hl-note", rows: "2", maxlength: String(LIMITS.note), placeholder: "Add a note",
    "aria-label": `Note for the highlight on page ${hl.page}`, "data-id": hl.id,
  }, session.hl.dirty.get(hl.id) ?? hl.note);
  note.addEventListener("input", () => noteEdited(session, hl.id, note.value));
  note.addEventListener("blur", () => saveNote(session, hl.id));
  return h("li", { class: "hl-item", "data-id": hl.id },
    h("button", { class: "hl-quote", type: "button", title: "Go to this highlight", onclick: () => jump(session, hl) }, hl.text),
    h("span", { class: "hl-page" }, `p. ${hl.page}`),
    note,
    h("button", { class: "btn hl-delete", type: "button", onclick: () => deleteHighlight(session, hl.id) }, "Delete"));
}

function renderPanel(session) {
  if (!session.hl.open) return;
  $("hl-damaged").hidden = !session.hl.notice;
  renderExport(session);
  const active = document.activeElement;
  const keep = active?.classList.contains("hl-note")
    ? { id: active.dataset.id, start: active.selectionStart, end: active.selectionEnd } : null;
  $("hl-empty").hidden = session.hl.list.length > 0;
  $("hl-list").replaceChildren(...session.hl.list.map((hl) => item(session, hl)));
  const box = keep && $("hl-list").querySelector(`textarea[data-id="${keep.id}"]`);
  if (box) {
    box.focus();
    box.setSelectionRange(keep.start, keep.end);
  }
}

// ---- export folder row ----

function renderExport(session) {
  const { exportFolder: folder, exportError: error } = session.hl;
  const [text, label] = folder
    ? [`Export to: ${middleTruncate(folder, PATH_MAX)}`, "Change"]
    : ["Export to Obsidian or Markdown", "Choose folder"];
  $("hl-export").replaceChildren(
    h("div", { class: "hl-export-row" },
      h("span", { class: "hl-export-path", title: folder ?? "" }, text),
      h("button", { class: "btn", type: "button", onclick: () => pickExportFolder(session) }, label)),
    error ? h("p", { class: "hl-export-error" }, `Export failed: ${errorWords(error)}`) : null);
}

async function pickExportFolder(session) {
  try {
    const path = await openDialog({ directory: true });
    if (!path || !live(session)) return;
    const res = await invoke("set_export_folder", { path });
    if (!live(session)) return;
    Object.assign(session.hl, { exportFolder: res.exportFolder, exportError: null });
    renderExport(session);
  } catch (code) {
    console.error("set_export_folder failed", code);
    if (live(session)) showBar(code === "not_a_folder" ? "That is not a folder." : "Could not use that folder.");
  }
}

// A book's export finished in the background; the line shows only for the open book.
window.__TAURI__.event.listen("highlights-exported", (e) => {
  const session = ctx?.current();
  if (!session?.hl || e.payload.id !== session.id) return;
  session.hl.exportError = e.payload.exportError ?? null;
  if (session.hl.open) renderExport(session);
}).catch((err) => console.error("export events not heard", err));

// ---- notes, delete, undo ----

function noteEdited(session, id, text) {
  const { dirty, timers } = session.hl;
  dirty.set(id, text);
  clearTimeout(timers.get(id));
  timers.set(id, setTimeout(() => saveNote(session, id), NOTE_MS));
}

// The local list is the truth for notes, so the reply is not applied.
function saveNote(session, id) {
  const { dirty, timers, list } = session.hl;
  clearTimeout(timers.get(id));
  timers.delete(id);
  // Not `live`: leaving the book flushes notes while the session is exiting.
  if (!dirty.has(id) || ctx.current() !== session) return;
  const note = dirty.get(id);
  dirty.delete(id);
  const hl = list.find((x) => x.id === id);
  if (!hl) return;
  hl.note = note;
  call(session, "set_note", { highlightId: id, note }).catch((code) => {
    console.error("set_note failed", code);
    if (live(session)) {
      showBar(NOTE_NOT_SAVED);
      fetchHighlights(session);
    }
  });
}

export function flushNotes(session) {
  for (const id of [...(session.hl?.dirty.keys() ?? [])]) saveNote(session, id);
}

function deleteHighlight(session, id) {
  if (!live(session) || !session.hl.list.some((x) => x.id === id)) return;
  flushNotes(session);
  setList(session, session.hl.list.filter((x) => x.id !== id));
  showBar("Highlight deleted ·", () => restore(session));
  call(session, "delete_highlight", { highlightId: id })
    .then((res) => live(session) && setList(session, res.highlights))
    .catch((code) => {
      console.error("delete_highlight failed", code);
      if (live(session)) {
        showBar("Could not delete that highlight.");
        fetchHighlights(session);
      }
    });
}

function restore(session) {
  if (!live(session)) return;
  call(session, "restore_highlight")
    .then((res) => live(session) && setList(session, res.highlights))
    .catch((code) => {
      console.error("restore_highlight failed", code);
      if (live(session)) showBar("Could not bring that highlight back.");
    });
}

function openNote(session, id) {
  openPanel(session);
  const box = $("hl-list").querySelector(`textarea[data-id="${id}"]`);
  box?.scrollIntoView({ block: "nearest" });
  box?.focus();
}

// ---- click menu, in the viewer document ----

function closeMenu(v) {
  v.menu?.remove();
  v.menu = null;
}

function openMenu(session, v, hl, x, y) {
  closeMenu(v);
  const menu = v.doc.createElement("div");
  menu.className = "rx-hl-menu";
  menu.setAttribute("role", "menu");
  for (const [label, act] of [["Note", () => openNote(session, hl.id)], ["Delete", () => deleteHighlight(session, hl.id)]]) {
    const button = v.doc.createElement("button");
    button.type = "button";
    button.setAttribute("role", "menuitem");
    button.textContent = label;
    button.addEventListener("click", () => {
      closeMenu(v);
      act();
    });
    menu.append(button);
  }
  v.doc.body.append(menu);
  menu.style.left = `${Math.max(8, Math.min(x, v.win.innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, v.win.innerHeight - menu.offsetHeight - 8))}px`;
  v.menu = menu;
}

// A click on a highlighted spot, with no text selected. Links stay with PDF.js.
function onPageClick(session, v, e) {
  if (!live(session) || e.button !== 0 || !v.win.getSelection().isCollapsed) return;
  if (e.target.closest?.(".rx-hl-btn, .rx-hl-menu, .annotationLayer :is(section, a)")) return;
  const page = e.target.closest?.(".page[data-page-number]");
  const layer = page?.querySelector(":scope > .textLayer");
  const num = Number(page?.dataset.pageNumber);
  const pageView = layer && v.app.pdfViewer.getPageView(num - 1);
  if (!pageView) return;
  const [px, py] = pageMap(pageView, layer).toPdf(e.clientX, e.clientY);
  const hl = session.hl.list.find((x) => x.page === num && x.rects.some((r) => pointInRect(r, px, py)));
  if (hl) openMenu(session, v, hl, e.clientX, e.clientY);
}

// ---- leaving and Escape ----

// Called after Back has flushed the queue.
export function endHighlights(session) {
  closePanel(session);
  hideBar();
  invoke("forget_undo", { id: session.id }).catch((err) => console.error("forget_undo failed", err));
}

export function closePanelIfOpen(session) {
  if (!session?.hl?.open) return false;
  closePanel(session);
  return true;
}

// Escape with focus in the panel (a note field saves first, inside closePanel).
document.addEventListener("keydown", (e) => {
  const session = ctx?.current();
  if (e.key !== "Escape" || !session?.hl?.open || session.exiting) return;
  e.preventDefault();
  closePanel(session);
  session.frame.contentWindow.focus();
});

// ---- keys ----

// True when the key was used here. `H` with a selection highlights it; Escape
// first closes the Highlight button.
export function highlightKey(e, session) {
  const v = session?.hl?.v;
  if (!v || !live(session)) return false;
  if (e.key === "Escape" && (v.menu || !v.button.hidden)) {
    closeMenu(v);
    if (!v.button.hidden) v.win.getSelection().removeAllRanges();
    hideButton(v);
    e.preventDefault();
    e.stopPropagation();
    return true;
  }
  if ((e.key === "h" || e.key === "H") && !e.ctrlKey && !e.metaKey && !e.altKey && readSelection(v)) {
    e.preventDefault();
    e.stopPropagation();
    addFromSelection(session);
    return true;
  }
  return false;
}
