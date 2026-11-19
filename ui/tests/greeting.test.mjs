import test from "node:test";
import assert from "node:assert/strict";
import { greeting, shelfSummary } from "../lib/greeting.js";

test("greeting: by local hour", () => {
  for (const h of [5, 11]) assert.equal(greeting(h), "Good morning");
  for (const h of [12, 17]) assert.equal(greeting(h), "Good afternoon");
  for (const h of [18, 22]) assert.equal(greeting(h), "Good evening");
  for (const h of [23, 0, 4]) assert.equal(greeting(h), "Good night");
});

const b = (section) => ({ section });

test("shelfSummary: all three parts", () => {
  const books = [b("reading"), b("reading"), b("finished"), ...Array(11).fill(b("to-read"))];
  assert.equal(shelfSummary(books), "2 in progress · 1 finished · 14 on the shelf");
});

test("shelfSummary: zero parts are skipped", () => {
  assert.equal(shelfSummary([b("to-read"), b("to-read")]), "2 on the shelf");
  assert.equal(shelfSummary([b("reading"), b("to-read")]), "1 in progress · 2 on the shelf");
  assert.equal(shelfSummary([b("finished")]), "1 finished · 1 on the shelf");
});

test("shelfSummary: singular book and empty shelf", () => {
  assert.equal(shelfSummary([b("to-read")]), "1 on the shelf");
  assert.equal(shelfSummary([]), "");
});
