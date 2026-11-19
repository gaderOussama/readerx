// The nested toc as flat rows in reading order, each with its depth.
export function flattenToc(toc, depth = 0) {
  return (toc ?? []).flatMap((item) => [{ item, depth }, ...flattenToc(item.subitems, depth + 1)]);
}
