// Text goes in as text nodes, never as HTML.
export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (name === "class") el.className = value;
    else if (name.startsWith("on")) el.addEventListener(name.slice(2), value);
    else el.setAttribute(name, value);
  }
  el.append(...children.filter((c) => c != null));
  return el;
}
