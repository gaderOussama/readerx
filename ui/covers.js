import { needsProbe } from "./lib/covers.js";

const { invoke, convertFileSrc } = window.__TAURI__.core;

const COVER_WIDTH = 240;
const TIMEOUT_MS = 60000;
const here = (path) => new URL(path, import.meta.url).href;

// Ids whose probe failed (corrupt, password, timeout). Memory only: tried again next launch.
export const failed = new Set();

let pdfjs = null;
let running = false;

async function loadPdfjs() {
  if (!pdfjs) {
    pdfjs = await import("./vendor/pdfjs/build/pdf.mjs");
    pdfjs.GlobalWorkerOptions.workerSrc = here("vendor/pdfjs/build/pdf.worker.mjs");
  }
  return pdfjs;
}

// Page 1 as a 240 px wide PNG, plus the PDF title and page count. No onPassword
// handler is set, so a locked book rejects instead of asking.
async function probe(book) {
  const lib = await loadPdfjs();
  const task = lib.getDocument({
    url: convertFileSrc(book.path),
    cMapUrl: here("vendor/pdfjs/web/cmaps/"),
    standardFontDataUrl: here("vendor/pdfjs/web/standard_fonts/"),
    wasmUrl: here("vendor/pdfjs/web/wasm/"),
    iccUrl: here("vendor/pdfjs/web/iccs/"),
    disableFontFace: true, // glyphs drawn as paths: no font loading under the strict CSP
  });
  const timer = setTimeout(() => task.destroy(), TIMEOUT_MS);
  try {
    const doc = await task.promise;
    const page = await doc.getPage(1);
    const base = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: COVER_WIDTH / base.width });
    const canvas = document.createElement("canvas");
    canvas.width = COVER_WIDTH;
    canvas.height = Math.max(1, Math.round(viewport.height));
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#fff";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    await page.render({ canvasContext: ctx, canvas, viewport }).promise;
    const png = await new Promise((done) => canvas.toBlob(done, "image/png"));
    const title = (await doc.getMetadata()).info?.Title;
    // Meta first: a saved cover means the book is done and is never probed again.
    await invoke("set_meta", { id: book.id, metaTitle: typeof title === "string" ? title : null, pageCount: doc.numPages });
    await invoke("save_cover", { id: book.id, png: new Uint8Array(await png.arrayBuffer()) });
  } finally {
    clearTimeout(timer);
    task.destroy().catch(() => {});
  }
}

// One book at a time. Stops when the reader opens (`paused`) and when nothing is left;
// the caller starts it again. `onChange` runs after every book so the shelf can update.
export async function probeCovers({ books, paused, onChange }) {
  if (running) return;
  running = true;
  try {
    for (let book; !paused() && (book = books().find((b) => needsProbe(b, failed)));) {
      try {
        await probe(book);
      } catch (err) {
        console.warn("cover probe failed", book.path, err);
        failed.add(book.id);
      }
      try {
        await onChange();
      } catch (err) {
        console.error("shelf update failed", err);
      }
      // A cover that was saved but never shows up must not loop forever.
      if (books().some((b) => b.id === book.id && needsProbe(b, failed))) failed.add(book.id);
    }
  } finally {
    running = false;
  }
}
