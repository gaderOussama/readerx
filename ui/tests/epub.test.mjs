import test from "node:test";
import assert from "node:assert/strict";
import { columnCount, stepFontSize, themeStyles, isExternalHref } from "../lib/epub.js";

test("columnCount: exactly 1000 px is one column, 1001 px is two", () => {
  // Catches >= instead of > (the spec says "wider than 1000").
  assert.equal(columnCount(1000), 1);
  assert.equal(columnCount(1001), 2);
  assert.equal(columnCount(720), 1);
});

test("stepFontSize: moves one step along the list", () => {
  assert.equal(stepFontSize(100, 1), 115);
  assert.equal(stepFontSize(100, -1), 90);
  assert.equal(stepFontSize(130, 1), 160);
});

test("stepFontSize: clamps at both ends", () => {
  // Catches running off the array (undefined size).
  assert.equal(stepFontSize(200, 1), 200);
  assert.equal(stepFontSize(80, -1), 80);
});

test("stepFontSize: an unknown size snaps to 100 before stepping", () => {
  // Catches indexOf -1 turning "smaller" into index -2 or "bigger" into 80.
  assert.equal(stepFontSize(107, 1), 115);
  assert.equal(stepFontSize(undefined, -1), 90);
});

const colors = { text: "#2B2A28", bg: "#F4F1EA" };

test("themeStyles: original is only the font size rule", () => {
  // Catches original recoloring the book.
  assert.equal(themeStyles("original", colors, 115), "html { font-size: 115% !important }");
});

test("themeStyles: paper recolors text, backgrounds, borders and links", () => {
  assert.equal(
    themeStyles("paper", colors, 100),
    "html { font-size: 100% !important } "
      + "*:not(img, svg, video, image) { color: inherit !important; background-color: transparent !important; border-color: currentColor !important } "
      + "html, body { color: #2B2A28 !important; background: #F4F1EA !important } "
      + "a, a * { color: #2B2A28 !important }",
  );
});

test("themeStyles: only night gives images a white background", () => {
  // Catches the image rule leaking into paper, or missing from night.
  const night = themeStyles("night", { text: "#D8D5CF", bg: "#1E1E1E" }, 80);
  assert.ok(night.endsWith(" img, svg { background: #fff !important }"));
  assert.ok(night.startsWith("html { font-size: 80% !important }"));
  assert.ok(night.includes("html, body { color: #D8D5CF !important; background: #1E1E1E !important }"));
  assert.ok(!themeStyles("paper", colors, 100).includes("img, svg {"));
});

test("themeStyles: html and body background comes after the catch-all transparent rule", () => {
  // Same specificity, so the later rule wins; the other order makes the page transparent.
  const css = themeStyles("paper", colors, 100);
  assert.ok(css.indexOf("background-color: transparent") < css.indexOf("html, body {"));
});

test("isExternalHref: only absolute http, https and mailto", () => {
  assert.equal(isExternalHref("https://example.com/a"), true);
  assert.equal(isExternalHref("HTTP://example.com"), true);
  assert.equal(isExternalHref("mailto:a@b.c"), true);
});

test("isExternalHref: everything else is refused", () => {
  // Catches javascript:, ftp:, data:, file: and relative or odd hrefs getting through.
  for (const href of ["javascript:alert(1)", "ftp://x.org/f", "data:text/html,hi", "file:///c:/x", "chapter2.xhtml", "//example.com", "", null, undefined, " http://x.org"]) {
    assert.equal(isExternalHref(href), false, String(href));
  }
});
