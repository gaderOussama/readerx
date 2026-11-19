import test from "node:test";
import assert from "node:assert/strict";
import { flattenToc } from "../lib/toc.js";

test("flattenToc walks the nested toc in reading order with depth", () => {
  const c = { label: "1.1", href: "c" };
  const b = { label: "1", href: "b", subitems: [c] };
  const a = { label: "0", href: "a", subitems: null };
  assert.deepEqual(flattenToc([a, b]), [{ item: a, depth: 0 }, { item: b, depth: 0 }, { item: c, depth: 1 }]);
});

test("flattenToc gives no rows for a missing or empty toc", () => {
  assert.deepEqual(flattenToc(undefined), []);
  assert.deepEqual(flattenToc(null), []);
  assert.deepEqual(flattenToc([]), []);
});
