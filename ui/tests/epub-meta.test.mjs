import test from "node:test";
import assert from "node:assert/strict";
import { pickString, isProtected } from "../lib/epub-meta.js";

test("pickString: a string is trimmed; blank is null", () => {
  assert.equal(pickString("  Dune \n"), "Dune");
  assert.equal(pickString("   "), null);
  assert.equal(pickString(""), null);
});

test("pickString: a language map gives its first usable value", () => {
  assert.equal(pickString({ en: "Dune" }), "Dune");
  assert.equal(pickString({ en: " ", fr: "Dune FR" }), "Dune FR");
});

test("pickString: an object with name uses name, even when name is a language map", () => {
  assert.equal(pickString({ name: "Frank Herbert", role: "aut" }), "Frank Herbert");
  assert.equal(pickString({ name: { en: "Frank Herbert" } }), "Frank Herbert");
});

test("pickString: an array gives its first item that yields a string", () => {
  assert.equal(pickString([{ name: "A. One" }, "B. Two"]), "A. One");
  assert.equal(pickString(["", { name: { ja: "山田" } }]), "山田");
});

test("pickString: anything else is null", () => {
  assert.equal(pickString(undefined), null);
  assert.equal(pickString(null), null);
  assert.equal(pickString(42), null);
  assert.equal(pickString([]), null);
  assert.equal(pickString({}), null);
  assert.equal(pickString({ name: "" }), null);
});

const enc = (...algs) =>
  `<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">${algs
    .map((a) => `<EncryptedData><EncryptionMethod Algorithm="${a}"/></EncryptedData>`)
    .join("")}</encryption>`;
const IDPF = "http://www.idpf.org/2008/embedding";
const ADOBE = "http://ns.adobe.com/pdf/enc#RC";

test("isProtected: no rights and no encryption file is not protected", () => {
  assert.equal(isProtected(null, null), false);
});

test("isProtected: a rights.xml of any content is protected", () => {
  assert.equal(isProtected("<rights/>", null), true);
  assert.equal(isProtected("", null), true);
});

test("isProtected: only the two font obfuscation methods is not protected", () => {
  assert.equal(isProtected(null, enc(IDPF, ADOBE)), false);
});

test("isProtected: any other method is protected, also next to font ones", () => {
  assert.equal(isProtected(null, enc("http://www.w3.org/2001/04/xmlenc#aes128-cbc")), true);
  assert.equal(isProtected(null, enc(IDPF, "http://www.w3.org/2001/04/xmlenc#aes128-cbc")), true);
});

test("isProtected: single quotes and namespace prefixes are read", () => {
  assert.equal(isProtected(null, `<enc:EncryptionMethod Algorithm='urn:other'/>`), true);
  assert.equal(isProtected(null, `<enc:EncryptionMethod Algorithm='${IDPF}'/>`), false);
  assert.equal(isProtected(null, `<enc:EncryptionMethod xmlns:enc="x" Algorithm = "urn:other"/>`), true);
});

test("isProtected: an Algorithm attribute outside EncryptionMethod is ignored", () => {
  assert.equal(isProtected(null, `<EncryptedKey Algorithm="urn:other"/>`), false);
});
