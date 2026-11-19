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
