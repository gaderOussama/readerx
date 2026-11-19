import { h } from "./lib/dom.js";
import { flattenToc } from "./lib/toc.js";
import { currentEpubView } from "./reader.js";

const $ = (id) => document.getElementById(id);

const MIN_QUERY = 2;

// Which book the panel was built for, and the running search (a new number cancels the old one).
let shownView = null;
let searchRun = 0;
let searchIter = null;
let relocateView = null;

export const tocIsOpen = () => !$("toc-panel").hidden;

function showTab(tab) {
  for (const button of document.querySelectorAll("[data-toc-tab]")) {
    button.setAttribute("aria-pressed", String(button.dataset.tocTab === tab));
  }
  $("toc-contents").hidden = tab !== "contents";
  $("toc-search").hidden = tab !== "search";
  if (tab === "search") $("toc-query").focus();
}

function markCurrent(view) {
  const current = view.lastLocation?.tocItem;
  const rows = [...$("toc-list").children];
  // tocItem is the same object as in book.toc; href is the fallback.
  const mark = current && (rows.find((li) => li.tocItem === current) ?? rows.find((li) => current.href && li.tocItem.href === current.href));
  for (const li of rows) li.firstChild.setAttribute("aria-current", String(li === mark));
  mark?.scrollIntoView({ block: "nearest" });
}

function renderContents(view) {
  const rows = flattenToc(view.book?.toc);
  $("toc-none").hidden = rows.length > 0;
  $("toc-list").replaceChildren(...rows.map(({ item, depth }) => {
    const li = h("li", { class: "toc-item" },
      h("button", {
        class: "toc-link", type: "button", style: `padding-left: ${16 + depth * 16}px`,
        onclick: () => view.goTo(item.href),
      }, item.label?.trim() || "Untitled"));
    li.tocItem = item;
    return li;
  }));
  return rows.length > 0;
}

// Cancels the running search; resolves once its generator has stopped drawing marks.
function stopSearch() {
  searchRun++;
  const iter = searchIter;
  searchIter = null;
  return iter ? iter.return().catch(() => {}) : Promise.resolve();
}

function resetSearch() {
  stopSearch();
  $("toc-query").value = "";
  $("toc-status").textContent = "";
  $("toc-results").replaceChildren();
}

function openToc() {
  const view = currentEpubView();
  if (!view) return;
  if (view !== shownView) {
    shownView = view;
    resetSearch();
    relocateView?.removeEventListener("relocate", onRelocate);
    relocateView = view;
    view.addEventListener("relocate", onRelocate);
  }
  const hasToc = renderContents(view);
  $("toc-panel").hidden = false;
  $("toc-toggle").setAttribute("aria-expanded", "true");
  showTab(hasToc ? "contents" : "search");
  markCurrent(view);
}

function onRelocate() {
  if (tocIsOpen() && relocateView === currentEpubView()) markCurrent(relocateView);
}

// Safe to call when closed, or when the view is already gone.
export function closeToc() {
  const view = shownView;
  const stopped = stopSearch();
  $("toc-panel").hidden = true;
  $("toc-toggle").setAttribute("aria-expanded", "false");
  const clear = () => {
    try {
      view?.clearSearch();
    } catch (err) {
      console.error("clearSearch failed", err);
    }
  };
  clear();
  // A search still running may draw one more batch of marks before it stops.
  stopped.then(clear);
  shownView = null;
  relocateView?.removeEventListener("relocate", onRelocate);
  relocateView = null;
}

async function runSearch() {
  const view = currentEpubView();
  const query = $("toc-query").value.trim();
  if (!view || view !== shownView || query.length < MIN_QUERY) return;
  await stopSearch();
  if (view !== shownView) return;
  const run = ++searchRun;
  const status = $("toc-status");
  const results = $("toc-results");
  status.textContent = "Searching…";
  results.replaceChildren();
  let found = false;
  const iter = (searchIter = view.search({ query, matchCase: false }));
  try {
    for await (const item of iter) {
      if (run !== searchRun) return;
      if (item === "done") break;
      if (!item.subitems) continue;
      found = true;
      results.append(
        h("li", { class: "toc-chapter" }, item.label?.trim() || "Untitled"),
        ...item.subitems.map(({ cfi, excerpt: { pre, match, post } }) => h("li", { class: "toc-match" },
          h("button", { class: "toc-link", type: "button", onclick: () => view.goTo(cfi) },
            pre, h("strong", {}, match), post))));
    }
    status.textContent = found ? "" : "No matches";
  } catch (err) {
    console.error("search failed", err);
    if (run === searchRun) status.textContent = "";
  }
}

$("toc-toggle").addEventListener("click", () => (tocIsOpen() ? closeToc() : openToc()));
for (const button of document.querySelectorAll("[data-toc-tab]")) {
  button.addEventListener("click", () => showTab(button.dataset.tocTab));
}
$("toc-query").addEventListener("keydown", (e) => {
  if (e.key === "Enter") runSearch();
});
