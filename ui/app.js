import { relativeDay, progressText } from "./lib/format.js";

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

function openBook() {}

function cover(book) {
  const tile = () => h("div", { class: "tile" }, h("span", {}, book.displayTitle));
  if (!book.coverPath) return h("div", { class: "cover" }, tile());
  const img = h("img", { src: convertFileSrc(book.coverPath), alt: "", loading: "lazy" });
  const box = h("div", { class: "cover" }, img);
  img.addEventListener("error", () => box.replaceChildren(tile()));
  return box;
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
    h("button", { class: "continue-open", type: "button", "data-id": book.id, onclick: () => openBook(book) },
      cover(book),
      h("span", { class: "continue-body" },
        h("span", { class: "eyebrow" }, "Continue reading"),
        h("span", { class: "continue-title" }, book.displayTitle),
        ...bookMeta(book, now))));
}

function card(book, now) {
  return h("li", { class: "card" },
    h("button", { class: "card-open", type: "button", "data-id": book.id, onclick: () => openBook(book) },
      cover(book),
      h("span", { class: "card-title" }, book.displayTitle),
      ...bookMeta(book, now)));
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
    shelf.replaceChildren(...shelfContent());
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
  await listen("library-changed", async () => {
    await refreshState();
    render();
  });
  // Two frames: let the first screen paint before the hidden window appears.
  requestAnimationFrame(() => requestAnimationFrame(() => getCurrentWindow().show()));
}

start();
