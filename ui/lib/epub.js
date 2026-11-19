// Pure helpers for the EPUB reader (spec 0003).

export const FONT_SIZES = [80, 90, 100, 115, 130, 160, 200];

// Two columns only when the reader area is wider than 1000 px.
export const columnCount = (width) => (width > 1000 ? 2 : 1);

// One step along FONT_SIZES; an unknown size counts as 100, the ends clamp.
export function stepFontSize(current, dir) {
  const at = FONT_SIZES.includes(current) ? FONT_SIZES.indexOf(current) : FONT_SIZES.indexOf(100);
  return FONT_SIZES[Math.min(FONT_SIZES.length - 1, Math.max(0, at + dir))];
}

// Style text for the book's section documents, which can't see the app's CSS
// variables, so `colors` ({ text, bg }) are literal values. Original keeps the book's own colors.
export function themeStyles(theme, { text, bg }, fontSize) {
  const rules = [`html { font-size: ${fontSize}% !important }`];
  if (theme !== "original") {
    rules.push(
      "*:not(img, svg, video, image) { color: inherit !important; background-color: transparent !important; border-color: currentColor !important }",
      // After the rule above: same specificity, so the later one wins and keeps the page color.
      `html, body { color: ${text} !important; background: ${bg} !important }`,
      `a, a * { color: ${text} !important }`,
    );
    if (theme === "night") rules.push("img, svg { background: #fff !important }");
  }
  return rules.join(" ");
}

// Links that may leave through the system browser: absolute http, https, mailto.
export const isExternalHref = (href) => /^(https?:|mailto:)/i.test(href ?? "");

// Book pages become blob: documents, which do not get the app's CSP in WebView2,
// so each page is sanitized and carries its own CSP. No script-src: default-src 'none' blocks scripts.
export const PAGE_CSP = "default-src 'none'; img-src 'self' blob: data:; style-src 'self' blob: data: 'unsafe-inline'; font-src 'self' blob: data:; media-src 'self' blob: data:";

// Foliate rewrites (and the reader sanitizes) only these exact strings, so a document
// type in any other spelling would be served raw and is refused.
const EXACT_TYPES = ["application/xhtml+xml", "text/html", "image/svg+xml", "text/css"];
const FONT_TYPES = ["application/font-woff", "application/x-font-ttf", "application/x-font-otf", "application/vnd.ms-opentype", "application/font-sfnt"];

// Which manifest media types the loader may serve: documents, styles, images, fonts, audio, video.
export function allowResource(type) {
  if (typeof type !== "string") return false;
  const base = type.split(";")[0].trim().toLowerCase();
  if (EXACT_TYPES.includes(base)) return type === base;
  return /^(image|font|audio|video)\//.test(base) || FONT_TYPES.includes(base);
}

// Remote url() and @import cut out of CSS text (the page CSP is the real wall; this is the second one).
export function stripRemoteCss(css) {
  const remote = String.raw`(?:https?:)?\/\/`;
  return css
    .replace(new RegExp(String.raw`@import\s*(?:url\(\s*)?["']?\s*${remote}[^;{}]*;?`, "gi"), "")
    .replace(new RegExp(String.raw`url\(\s*(?:"\s*${remote}[^"]*"|'\s*${remote}[^']*'|${remote}[^)]*)\s*\)`, "gi"), 'url("")');
}

// foliate's paginator needs a <body>, so an SVG spine page would hang it; it goes inside an XHTML page.
export const wrapSvgPage = (svg) =>
  `<html xmlns="${XHTML_NS}"><head></head><body>${svg.replace(/^\s*<\?xml[^>]*\?>\s*/, "")}</body></html>`;

const BAD_TAGS =new Set(["script", "iframe", "frame", "object", "embed", "set", "animate", "animatemotion", "animatetransform", "handler", "listener", "base"]);
const URL_ATTRS = new Set(["src", "href", "poster", "data", "srcset", "background"]);
const XHTML_NS = "http://www.w3.org/1999/xhtml";
const squash = (value) => value.replace(/[\s\u0000-\u001f\u007f-\u009f]/g, "").toLowerCase();

// Strips everything in a parsed book page that can run code or reach the network, in place:
// script-like elements, on* and srcdoc attributes, javascript: values, remote resources,
// remote CSS. XHTML and HTML pages also get a CSP meta as the very first thing in the head.
export function sanitizePage(doc) {
  for (const el of [...doc.getElementsByTagName("*")]) {
    const tag = el.localName.toLowerCase();
    const refresh = tag === "meta" && el.getAttribute("http-equiv")?.trim().toLowerCase() === "refresh";
    if (BAD_TAGS.has(tag) || refresh) {
      el.remove();
      continue;
    }
    for (const attr of [...el.attributes]) {
      const name = attr.localName.toLowerCase();
      const value = squash(attr.value);
      const remote = URL_ATTRS.has(name) && /^(https?:|ftp:|(?:\/|\\){2})/.test(value) && !(name === "href" && tag === "a");
      if (name.startsWith("on") || name === "srcdoc" || remote || /^(javascript:|vbscript:|data:text\/html)/.test(value)) {
        el.removeAttributeNode(attr);
      } else if (name === "style") {
        attr.value = stripRemoteCss(attr.value);
      }
    }
    if (tag === "style") el.textContent = stripRemoteCss(el.textContent);
  }
  const root = doc.documentElement;
  if (root?.namespaceURI !== XHTML_NS) return;
  const head = doc.head ?? doc.createElementNS(XHTML_NS, "head");
  root.prepend(head);
  const meta = doc.createElementNS(XHTML_NS, "meta");
  meta.setAttribute("http-equiv", "Content-Security-Policy");
  meta.setAttribute("content", PAGE_CSP);
  head.prepend(meta);
}
