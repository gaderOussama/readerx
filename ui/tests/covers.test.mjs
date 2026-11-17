import test from "node:test";
import assert from "node:assert/strict";
import { needsProbe } from "../lib/covers.js";

const book = (over = {}) => ({ id: "a", present: true, coverPath: null, ...over });

test("needsProbe: a present book with no cover needs one", () => {
  assert.equal(needsProbe(book(), new Set()), true);
});

test("needsProbe: a book that has a cover is skipped", () => {
  assert.equal(needsProbe(book({ coverPath: "covers/a.png" }), new Set()), false);
});

test("needsProbe: a book that failed this session is skipped", () => {
  assert.equal(needsProbe(book(), new Set(["a"])), false);
  assert.equal(needsProbe(book({ id: "b" }), new Set(["a"])), true);
});

test("needsProbe: a book missing from the folder is skipped", () => {
  assert.equal(needsProbe(book({ present: false }), new Set()), false);
});
