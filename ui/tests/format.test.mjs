import test from "node:test";
import assert from "node:assert/strict";
import { relativeDay, progressText, epubProgressParts } from "../lib/format.js";

// Dates are built from local-time parts, so the tests hold in any time zone.
const local = (y, mo, d, h = 0, mi = 0, s = 0) => new Date(y, mo - 1, d, h, mi, s);
const iso = (date) => date.toISOString();

test("relativeDay: same calendar day is Today", () => {
  const now = local(2026, 11, 16, 23, 59, 59);
  assert.equal(relativeDay(iso(local(2026, 11, 16, 0, 0, 0)), now), "Today");
  assert.equal(relativeDay(iso(local(2026, 11, 16, 23, 59, 59)), now), "Today");
});

test("relativeDay: 31 seconds across midnight is Yesterday", () => {
  const now = local(2026, 11, 16, 0, 0, 30);
  assert.equal(relativeDay(iso(local(2026, 11, 15, 23, 59, 59)), now), "Yesterday");
  assert.equal(relativeDay(iso(local(2026, 11, 16, 0, 0, 1)), now), "Today");
});

test("relativeDay: start of yesterday is still Yesterday late in the day", () => {
  const now = local(2026, 11, 16, 23, 59, 59);
  assert.equal(relativeDay(iso(local(2026, 11, 15, 0, 0, 0)), now), "Yesterday");
  assert.equal(relativeDay(iso(local(2026, 11, 14, 23, 59, 59)), now), "2 days ago");
});

test("relativeDay: 2 to 6 days ago, then a date", () => {
  const now = local(2026, 11, 16, 12);
  assert.equal(relativeDay(iso(local(2026, 11, 14, 12)), now), "2 days ago");
  assert.equal(relativeDay(iso(local(2026, 11, 10, 12)), now), "6 days ago");
  assert.equal(relativeDay(iso(local(2026, 11, 9, 12)), now), "9 Nov 2026");
});

test("relativeDay: counts calendar days, not 24 h spans", () => {
  const now = local(2026, 11, 16, 0, 0, 30);
  // 6 calendar days back, though only 5 days 23 h 59 m passed
  assert.equal(relativeDay(iso(local(2026, 11, 10, 0, 1, 0)), now), "6 days ago");
  // 7 calendar days back, though only 6 days 23 h 59 m passed
  assert.equal(relativeDay(iso(local(2026, 11, 9, 0, 1, 0)), now), "9 Nov 2026");
});

test("relativeDay: day number has no zero padding", () => {
  const now = local(2026, 11, 16, 12);
  assert.equal(relativeDay(iso(local(2026, 11, 3, 8)), now), "3 Nov 2026");
});

test("relativeDay: month and year boundaries", () => {
  assert.equal(relativeDay(iso(local(2026, 10, 31, 23, 59)), local(2026, 11, 1, 0, 1)), "Yesterday");
  assert.equal(relativeDay(iso(local(2025, 12, 31, 23, 0)), local(2026, 1, 1, 1, 0)), "Yesterday");
  assert.equal(relativeDay(iso(local(2025, 12, 25, 10)), local(2026, 1, 5, 10)), "25 Dec 2025");
});

test("relativeDay: a DST change does not shift the day count", () => {
  // EU and US clocks change in these windows; calendar days must still be 2.
  assert.equal(relativeDay(iso(local(2026, 3, 28, 12)), local(2026, 3, 30, 12)), "2 days ago");
  assert.equal(relativeDay(iso(local(2026, 11, 1, 12)), local(2026, 11, 3, 12)), "2 days ago");
});

test("relativeDay: a future date reads Today", () => {
  const now = local(2026, 11, 16, 12);
  assert.equal(relativeDay(iso(local(2026, 11, 16, 12, 0, 1)), now), "Today");
  assert.equal(relativeDay(iso(local(2026, 11, 17, 0, 0, 0)), now), "Today");
  assert.equal(relativeDay(iso(local(2030, 1, 1)), now), "Today");
});

test("relativeDay: fixed English month names", () => {
  const now = local(2027, 12, 31, 12);
  const names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  names.forEach((name, i) => {
    assert.equal(relativeDay(iso(local(2026, i + 1, 15, 12)), now), `15 ${name} 2026`);
  });
});

test("progressText: page, count and rounded percent", () => {
  const book = { position: { page: 3 }, pageCount: 10, progress: 0.3 };
  assert.equal(progressText(book), "p. 3 / 10 · 30%");
});

test("progressText: rounds to the nearest percent", () => {
  const at = (progress) => progressText({ position: { page: 1 }, pageCount: 100, progress });
  assert.equal(at(0.124), "p. 1 / 100 · 12%");
  assert.equal(at(0.126), "p. 1 / 100 · 13%");
  assert.equal(at(0), "p. 1 / 100 · 0%");
  assert.equal(at(1), "p. 1 / 100 · 100%");
});

test("progressText: null without a position or a page count", () => {
  assert.equal(progressText({ position: null, pageCount: 10, progress: 0 }), null);
  assert.equal(progressText({ position: { page: 3 }, pageCount: null, progress: 0.3 }), null);
  assert.equal(progressText({ position: null, pageCount: null, progress: 0 }), null);
});

test("epubProgressParts: chapter and rounded percent as separate parts", () => {
  const book = { position: { kind: "epub", chapter: "Chapter 3" }, progress: 0.344 };
  assert.deepEqual(epubProgressParts(book), { chapter: "Chapter 3", percent: "34%" });
  assert.equal(epubProgressParts({ ...book, progress: 0.345 }).percent, "35%");
});

test("epubProgressParts: no chapter gives a null chapter, not an empty one", () => {
  const at = (chapter) => epubProgressParts({ position: { kind: "epub", chapter }, progress: 0.5 });
  assert.deepEqual(at(null), { chapter: null, percent: "50%" });
  assert.equal(at("").chapter, null);
});

test("epubProgressParts: null for an unopened book", () => {
  assert.equal(epubProgressParts({ position: null, progress: 0 }), null);
});
