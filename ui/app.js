const { invoke } = window.__TAURI__.core;
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
    shelf.replaceChildren(h("ul", { class: "title-list" },
      ...state.books.map((book) => h("li", {}, book.displayTitle))));
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
