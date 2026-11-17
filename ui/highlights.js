import {
  LIMITS, cleanText, normalizeRect, rectsIntersect, mergeLineRects, percentBox, sortHighlights,
} from "./lib/highlights.js";

const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);

const TOO_LONG = "That highlight is too long.";
const NOT_SAVED = "Could not save that highlight.";
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
  session.hl = { list: [], v: null };
  fetchHighlights(session);
}

function setList(session, list) {
  session.hl.list = sortHighlights(list);
  redraw(session);
}

async function fetchHighlights(session) {
  try {
    const res = await call(session, "get_highlights");
    if (live(session)) setList(session, res.highlights);
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
  const v = { win, doc, app, button, down: false, timer: 0 };
  session.hl.v = v;

  doc.addEventListener("mousedown", () => { v.down = true; hideButton(v); }, true);
  doc.addEventListener("mouseup", () => {
    v.down = false;
    showButton(v);
  }, true);
  doc.addEventListener("selectionchange", () => {
    hideButton(v);
    clearTimeout(v.timer);
    if (!v.down) v.timer = setTimeout(() => showButton(v), 200);
  });
  app.pdfViewer.container.addEventListener("scroll", () => hideButton(v), { passive: true });
  app.eventBus.on("scalechanging", () => hideButton(v));
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

// ---- keys ----

// True when the key was used here. `H` with a selection highlights it; Escape
// first closes the Highlight button.
export function highlightKey(e, session) {
  const v = session?.hl?.v;
  if (!v || !live(session)) return false;
  if (e.key === "Escape" && !v.button.hidden) {
    v.win.getSelection().removeAllRanges();
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
