import test from "node:test";
import assert from "node:assert/strict";
import {
  cleanText, normalizeRect, rectsIntersect, pointInRect, mergeLineRects,
  percentBox, sortHighlights, middleTruncate, errorWords,
} from "../lib/highlights.js";

test("cleanText: breaks, tabs and no-break spaces become single spaces", () => {
  assert.equal(cleanText("a\nb\r\nc\td\u00A0e"), "a b c d e");
});

test("cleanText: soft hyphens go, runs of spaces collapse, ends are trimmed", () => {
  assert.equal(cleanText("  co\u00ADoper\u00ADate   now \n "), "cooperate now");
  assert.equal(cleanText("\n\t "), "");
});

test("normalizeRect: min and max of the two corners", () => {
  assert.deepEqual(normalizeRect([10, 20, 5, 8]), [5, 8, 10, 20]);
  assert.deepEqual(normalizeRect([1, 2, 3, 4]), [1, 2, 3, 4]);
});

test("rectsIntersect: overlap only, touching edges do not count", () => {
  assert.equal(rectsIntersect([0, 0, 10, 10], [5, 5, 15, 15]), true);
  assert.equal(rectsIntersect([0, 0, 10, 10], [10, 0, 20, 10]), false);
  assert.equal(rectsIntersect([0, 0, 10, 10], [0, 11, 10, 20]), false);
  assert.equal(rectsIntersect([0, 0, 10, 10], [2, 2, 3, 3]), true);
});

test("pointInRect: edges count as inside", () => {
  assert.equal(pointInRect([0, 0, 10, 10], 10, 10), true);
  assert.equal(pointInRect([0, 0, 10, 10], 10.1, 5), false);
});

test("mergeLineRects: same line within the gap merges", () => {
  const out = mergeLineRects([[0, 10, 20, 22], [22, 10.5, 40, 21.5], [41, 10, 60, 22]]);
  assert.deepEqual(out, [[0, 10, 60, 22]]);
});

test("mergeLineRects: a gap of 4 or more, or another line, stays apart", () => {
  assert.equal(mergeLineRects([[0, 10, 20, 22], [24, 10, 40, 22]]).length, 2);
  assert.equal(mergeLineRects([[0, 10, 20, 22], [0, 30, 20, 42]]).length, 2);
  assert.equal(mergeLineRects([[0, 10, 20, 22], [20, 13, 40, 22]]).length, 2);
});

test("mergeLineRects: vertical lines (rotated pages) merge the same way", () => {
  const out = mergeLineRects([[10, 0, 22, 20], [10, 22, 22, 40]]);
  assert.deepEqual(out, [[10, 0, 22, 40]]);
});

test("mergeLineRects: empty boxes are dropped", () => {
  assert.deepEqual(mergeLineRects([[5, 5, 5, 9], [0, 0, 10, 10]]), [[0, 0, 10, 10]]);
  assert.deepEqual(mergeLineRects([]), []);
});

test("percentBox: viewport rect to percentages of the page, any corner order", () => {
  assert.deepEqual(percentBox([100, 50, 300, 100], 1000, 500), { left: 10, top: 10, width: 20, height: 10 });
  assert.deepEqual(percentBox([300, 100, 100, 50], 1000, 500), { left: 10, top: 10, width: 20, height: 10 });
});

test("sortHighlights: page, then higher on the page, then further left", () => {
  const hl = (id, page, x1, y2) => ({ id, page, rects: [[x1, y2 - 10, x1 + 5, y2]] });
  const list = [hl("d", 2, 10, 700), hl("c", 1, 50, 500), hl("b", 1, 10, 500), hl("a", 1, 99, 600)];
  assert.deepEqual(sortHighlights(list).map((h) => h.id), ["a", "b", "c", "d"]);
  assert.deepEqual(list.map((h) => h.id), ["d", "c", "b", "a"]);
});

test("middleTruncate: short text stays, long text keeps both ends", () => {
  assert.equal(middleTruncate("C:\\Vault", 20), "C:\\Vault");
  const out = middleTruncate("C:\\Users\\someone\\Documents\\Vault\\Books", 20);
  assert.equal(out.length, 20);
  assert.ok(out.startsWith("C:\\Users"));
  assert.ok(out.endsWith("\\Books"));
  assert.ok(out.includes("…"));
});

test("errorWords: export codes in words", () => {
  assert.equal(errorWords("folder_missing"), "folder not found");
  assert.equal(errorWords("locked"), "the file is open in another program");
  assert.equal(errorWords("write_failed"), "could not write the file");
  assert.equal(errorWords("other"), "unknown error");
});
