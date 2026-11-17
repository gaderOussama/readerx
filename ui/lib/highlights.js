export const LIMITS = { rects: 200, text: 5000, note: 10000 };

const EXPORT_WORDS = {
  folder_missing: "folder not found",
  locked: "the file is open in another program",
  write_failed: "could not write the file",
};

export const errorWords = (code) => EXPORT_WORDS[code] ?? "unknown error";

export function cleanText(text) {
  return text
    .replace(/­/g, "")
    .replace(/[\r\n\t ]/g, " ")
    .replace(/ {2,}/g, " ")
    .trim();
}

export const normalizeRect = ([x1, y1, x2, y2]) =>
  [Math.min(x1, x2), Math.min(y1, y2), Math.max(x1, x2), Math.max(y1, y2)];

// Overlap only: boxes that just touch do not intersect.
export const rectsIntersect = (a, b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];

export const pointInRect = (r, x, y) => x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3];

const near = (a, b, tol) => Math.abs(a - b) <= tol;

// Same line: top and bottom within `tol`, gap under `gap`. Rotated pages have
// vertical lines, so the same test runs on the other axis.
function sameLine(a, b, tol, gap) {
  const across = near(a[1], b[1], tol) && near(a[3], b[3], tol) && b[0] - a[2] < gap && a[0] - b[2] < gap;
  const down = near(a[0], b[0], tol) && near(a[2], b[2], tol) && b[1] - a[3] < gap && a[1] - b[3] < gap;
  return across || down;
}

export function mergeLineRects(rects, tol = 2, gap = 4) {
  const out = [];
  for (const r of rects) {
    if (r[2] - r[0] <= 0 || r[3] - r[1] <= 0) continue;
    const last = out[out.length - 1];
    if (last && sameLine(last, r, tol, gap)) {
      out[out.length - 1] = [Math.min(last[0], r[0]), Math.min(last[1], r[1]), Math.max(last[2], r[2]), Math.max(last[3], r[3])];
    } else {
      out.push(r);
    }
  }
  return out;
}

// A viewport rectangle (any corner order) as percentages of the page size.
export function percentBox([x1, y1, x2, y2], width, height) {
  return {
    left: (Math.min(x1, x2) / width) * 100,
    top: (Math.min(y1, y2) / height) * 100,
    width: (Math.abs(x2 - x1) / width) * 100,
    height: (Math.abs(y2 - y1) / height) * 100,
  };
}

// Page, then higher on the page (larger top y in PDF units), then further left.
export function sortHighlights(list) {
  const first = (h) => h.rects[0];
  return [...list].sort((a, b) => a.page - b.page || first(b)[3] - first(a)[3] || first(a)[0] - first(b)[0]);
}

export function middleTruncate(text, max) {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.floor(keep / 2);
  return `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
}
