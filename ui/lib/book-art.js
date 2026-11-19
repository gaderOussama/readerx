import { s } from "./dom.js";

const lines = (x1, x2, ys) => ys.map((y) => s("path", { class: "ln", d: `M${x1} ${y}H${x2}` }));

// Line-art open book. The right page turns over and settles (CSS in styles.css).
// Without motion it stays a still, open book.
export function bookArt() {
  return s("svg", { class: "book-art", viewBox: "0 0 220 140", width: "220", height: "140", "aria-hidden": "true", focusable: "false" },
    s("path", { d: "M110 40C88 33 50 33 18 42V118C50 109 88 109 110 116C132 109 170 109 202 118V42C170 33 132 33 110 40Z" }),
    s("path", { d: "M110 34C90 28 52 28 26 36V108C52 100 90 100 110 108Z" }),
    ...lines(38, 98, [52, 62, 72, 82]),
    s("path", { d: "M110 34C130 28 168 28 194 36V108C168 100 130 100 110 108Z" }),
    s("g", { class: "book-turn" },
      s("path", { d: "M110 34C130 28 168 28 194 36V108C168 100 130 100 110 108Z" }),
      ...lines(122, 182, [52, 62, 72, 82])));
}
