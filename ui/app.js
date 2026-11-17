import { relativeDay, progressText } from "./lib/format.js";
import { openReader } from "./reader.js";
import { failed } from "./covers.js";

const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;
const { open: openDialog } = window.__TAURI__.dialog;

const $ = (id) => document.getElementById(id);

// Text goes in as text nodes, never as HTML.
function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (name === "class") el.className = value;
    else if (name.startsWith("on")) el.addEventListener(name.slice(2), value);
    else el.setAttribute(name, value);
  }
  el.append(...children.filter((c) => c != null));
  return el;
}

let state = null;
let localNotice = null; // errors raised here, not by Rust
let readerOpen = false;
let shelfScroll = 0;

async function openBook(book) {
  if (readerOpen) return;
  readerOpen = true;
  closeMenu();
  let opened;
  try {
    opened = await invoke("open_book", { id: book.id });
  } catch (code) {
    readerOpen = false;
    showError(code === "not_present" ? "That book is no longer in your folder." : `Could not open that book (${code}).`);
    await refreshState();
    render();
    return;
  }
  shelfScroll = $("shelf-view").scrollTop;
  $("shelf-view").hidden = true;
  openReader({
    book,
    path: opened.path,
    position: opened.position,
    theme: state.settings.pageTheme,
    onExit: closeReader,
    onTheme: (theme) => {
      state.settings.pageTheme = theme;
      applyTheme(theme);
    },
  });
}

async function closeReader() {
  try {
    await refreshState();
  } catch (code) {
    console.error("get_state failed", code);
  }
  readerOpen = false;
  $("shelf-view").hidden = false;
  render();
  $("shelf-view").scrollTop = shelfScroll;
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
}

async function refreshState() {
  state = await invoke("get_state");
  applyTheme(state.settings.pageTheme);
}

function showError(message) {
  localNotice = message;
  renderNotice();
}

function renderNotice() {
  const text = localNotice ?? state?.notice ?? null;
  $("notice").hidden = text == null;
  $("notice-text").textContent = text ?? "";
}

async function dismissNotice() {
  if (localNotice != null) {
    localNotice = null;
  } else {
    await invoke("dismiss_notice");
    state.notice = null;
  }
  renderNotice();
}

async function pickFolder() {
  const path = await openDialog({ directory: true });
  if (!path) return;
  try {
    state = await invoke("choose_folder", { path });
    localNotice = null;
    render();
  } catch (code) {
    showError(code === "not_a_folder" ? "That is not a folder." : `Could not use that folder (${code}).`);
  }
}

const SECTIONS = [
  ["reading", "Reading"],
  ["to-read", "To read"],
  ["finished", "Finished"],
];

// Loaded covers are moved into the next render, so a re-render does not reload or flash them.
const coverImgs = new Map();

function cover(book) {
  const tile = (note) => h("div", { class: "tile" }, h("span", {}, book.displayTitle),
    note ? h("small", { class: "tile-note" }, note) : null);
  if (!book.coverPath) return h("div", { class: "cover" }, tile(failed.has(book.id) ? "Can't preview" : null));
  let img = coverImgs.get(book.coverPath);
  if (!img) {
    img = h("img", { src: convertFileSrc(book.coverPath), alt: "", loading: "lazy" });
    img.addEventListener("error", () => {
      coverImgs.delete(book.coverPath);
      img.parentNode?.replaceChildren(tile());
    });
    coverImgs.set(book.coverPath, img);
  }
  return h("div", { class: "cover" }, img);
}

function bookMeta(book, now) {
  const progress = progressText(book);
  const date = book.lastOpenedAt ? relativeDay(book.lastOpenedAt, now) : null;
  return [
    progress ? h("span", { class: "meta" }, progress) : null,
    date ? h("span", { class: "meta" }, date) : null,
  ];
}

function continueCard(book, now) {
  return h("section", { class: "continue", "aria-label": "Continue reading" },
    h("button", { class: "continue-open", type: "button", "data-key": `open:${book.id}`, onclick: () => openBook(book) },
      cover(book),
      h("span", { class: "continue-body" },
        h("span", { class: "eyebrow" }, "Continue reading"),
        h("span", { class: "continue-title" }, book.displayTitle),
        ...bookMeta(book, now))),
    ...cardMenu(book));
}

let openMenu = null; // { button, menu }

function closeMenu() {
  if (!openMenu) return;
  openMenu.menu.hidden = true;
  openMenu.button.setAttribute("aria-expanded", "false");
  openMenu = null;
}

document.addEventListener("click", (e) => {
  if (openMenu && !openMenu.menu.contains(e.target) && !openMenu.button.contains(e.target)) closeMenu();
});
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || !openMenu) return;
  openMenu.button.focus();
  closeMenu();
});

async function setFinished(book, finished) {
  closeMenu();
  try {
    await invoke("set_finished", { id: book.id, finished });
    await refreshState();
    render();
  } catch (code) {
    showError(`Could not update that book (${code}).`);
  }
}

function cardMenu(book) {
  const finished = book.finishedAt != null;
  const item = h("button", { class: "menu-item", type: "button", role: "menuitem",
    onclick: () => setFinished(book, !finished) }, finished ? "Mark as unfinished" : "Mark as finished");
  const menu = h("div", { class: "card-menu", role: "menu", hidden: "" }, item);
  const button = h("button", { class: "card-menu-btn", type: "button", "aria-haspopup": "menu",
    "aria-expanded": "false", "aria-label": `Options for ${book.displayTitle}`, "data-key": `menu:${book.id}`,
    onclick: () => {
      const wasOpen = openMenu?.menu === menu;
      closeMenu();
      if (wasOpen) return;
      menu.hidden = false;
      button.setAttribute("aria-expanded", "true");
      openMenu = { button, menu };
      item.focus();
    } }, "⋯");
  return [button, menu];
}

function card(book, now) {
  return h("li", { class: "card" },
    h("button", { class: "card-open", type: "button", "data-key": `open:${book.id}`, onclick: () => openBook(book) },
      cover(book),
      h("span", { class: "card-title" }, book.displayTitle),
      ...bookMeta(book, now)),
    ...cardMenu(book));
}

function shelfContent() {
  const now = new Date();
  const nodes = [];
  const current = state.books.find((b) => b.id === state.continueId);
  if (current) nodes.push(continueCard(current, now));
  for (const [section, label] of SECTIONS) {
    const books = state.books.filter((b) => b.section === section && b.id !== state.continueId);
    if (books.length === 0) continue;
    nodes.push(h("section", { class: "section" },
      h("h2", { class: "section-title" }, label),
      h("ul", { class: "grid" }, ...books.map((b) => card(b, now)))));
  }
  return nodes;
}

function emptyState(title, text, buttonLabel) {
  return h("section", { class: "empty-state" },
    h("h2", {}, title),
    text ? h("p", {}, text) : null,
    h("button", { class: "btn", type: "button", onclick: pickFolder }, buttonLabel));
}

function render() {
  closeMenu();
  const { folderStatus, settings } = state;
  $("change-folder").hidden = folderStatus === "unset";
  renderNotice();

  const shelf = $("shelf");
  if (folderStatus === "unset") {
    shelf.replaceChildren(emptyState("Welcome to ReaderX",
      "Choose the folder that holds your PDFs. ReaderX will keep your place in every book.",
      "Choose folder"));
  } else if (folderStatus === "missing") {
    shelf.replaceChildren(emptyState("Books folder not found", settings.booksFolder, "Choose folder"));
  } else if (folderStatus === "empty") {
    shelf.replaceChildren(emptyState(`No PDFs found in ${settings.booksFolder}`, null, "Change folder"));
  } else {
    const focusedKey = document.activeElement?.dataset?.key;
    shelf.replaceChildren(...shelfContent());
    if (focusedKey) shelf.querySelector(`[data-key="${CSS.escape(focusedKey)}"]`)?.focus();
  }
}

async function start() {
  $("change-folder").addEventListener("click", pickFolder);
  $("notice-dismiss").addEventListener("click", dismissNotice);
  try {
    await refreshState();
    render();
  } catch (code) {
    $("shelf").replaceChildren(emptyState("Could not load your library", String(code), "Choose folder"));
  }
  // Two frames: let the first screen paint before the hidden window appears.
  requestAnimationFrame(() => requestAnimationFrame(() => getCurrentWindow().show()));
  await listen("library-changed", async () => {
    try {
      await refreshState();
      if (!readerOpen) render();
    } catch (code) {
      console.error("get_state failed", code);
    }
  });
}

start();
