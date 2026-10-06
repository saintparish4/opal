// The header on every page: the menu on narrow screens, and search.
//
// The menu's button opens and closes the panel, and choosing a link or
// pressing Escape closes it.
const menuButton = document.querySelector(".menu-button");
const menu = document.getElementById("mobile-menu");

function setMenu(open) {
  menu.hidden = !open;
  menuButton.setAttribute("aria-expanded", String(open));
  menuButton.setAttribute("aria-label", open ? "Close menu" : "Open menu");
}

menuButton.addEventListener("click", () => setMenu(menu.hidden));
menu.addEventListener("click", (event) => {
  if (event.target.closest("a")) setMenu(false);
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !menu.hidden) {
    setMenu(false);
    menuButton.focus();
  }
});
// The panel only exists below the width where the links are hidden; a window
// widened past it shouldn't keep one open underneath them.
window.matchMedia("(min-width: 821px)").addEventListener("change", (event) => {
  if (event.matches) setMenu(false);
});

// Search. The index is a second file, fetched the first time the dialog
// opens, so a visit that never searches never loads it.
const siteRoot = new URL(".", document.currentScript.src);
let searchDialog;
let searchIndex;

function loadIndex() {
  if (searchIndex) return searchIndex;
  searchIndex = new Promise((resolve) => {
    const script = document.createElement("script");
    script.src = new URL("search-index.js", siteRoot);
    script.onload = () => resolve(window.OPAL_SEARCH || []);
    script.onerror = () => resolve([]);
    document.head.append(script);
  });
  return searchIndex;
}

// Every word of the query has to appear. A word in the title counts for more
// than one in the text, and a title that starts with it for more again.
function search(entries, query) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  return entries
    .map((entry) => {
      const title = entry.title.toLowerCase();
      const text = entry.text.toLowerCase();
      let score = 0;
      for (const word of words) {
        if (title.startsWith(word)) score += 6;
        else if (title.includes(word)) score += 4;
        else if (text.includes(word)) score += 1;
        else return null;
      }
      return { entry, score };
    })
    .filter(Boolean)
    .sort((a, b) => b.score - a.score)
    .slice(0, 8)
    .map(({ entry }) => entry);
}

// A few words either side of the first place the query appears.
function snippet(text, query) {
  const word = query.toLowerCase().split(/\s+/).find(Boolean) || "";
  const at = text.toLowerCase().indexOf(word);
  const from = Math.max(0, at - 50);
  return (from > 0 ? "…" : "") + text.slice(from, from + 130).trim() + (from + 130 < text.length ? "…" : "");
}

function buildSearch() {
  const dialog = document.createElement("dialog");
  dialog.className = "search";
  dialog.setAttribute("aria-label", "Search");
  dialog.innerHTML = `
    <input type="search" placeholder="Search sections and posts…" aria-label="Search" autocomplete="off" spellcheck="false">
    <ol class="search-results" aria-live="polite"></ol>
    <p class="search-hint">↑ ↓ to move · Enter to open · Esc to close</p>`;
  document.body.append(dialog);
  const input = dialog.querySelector("input");
  const list = dialog.querySelector(".search-results");
  let chosen = 0;

  const mark = () =>
    [...list.querySelectorAll("a")].forEach((link, at) => link.classList.toggle("chosen", at === chosen));

  async function draw() {
    const query = input.value.trim();
    const found = search(await loadIndex(), query);
    chosen = 0;
    list.replaceChildren(
      ...found.map((entry) => {
        const item = document.createElement("li");
        const link = document.createElement("a");
        link.href = new URL(entry.url, siteRoot);
        const kind = document.createElement("span");
        kind.className = "search-kind";
        kind.textContent = entry.kind;
        const title = document.createElement("strong");
        title.textContent = entry.title;
        const text = document.createElement("span");
        text.className = "search-text";
        text.textContent = snippet(entry.text, query);
        link.append(kind, title, text);
        item.append(link);
        return item;
      }),
    );
    if (query && !found.length) {
      const none = document.createElement("li");
      none.className = "search-none";
      none.textContent = `Nothing matches "${query}"`;
      list.append(none);
    }
    mark();
  }

  input.addEventListener("input", draw);
  input.addEventListener("keydown", (event) => {
    const links = [...list.querySelectorAll("a")];
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!links.length) return;
      chosen = (chosen + (event.key === "ArrowDown" ? 1 : -1) + links.length) % links.length;
      mark();
      links[chosen].scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter" && links[chosen]) {
      event.preventDefault();
      links[chosen].click();
    }
  });
  // A result on this page only moves the hash, so the dialog has to go too.
  list.addEventListener("click", (event) => {
    if (event.target.closest("a")) dialog.close();
  });
  // A click on the backdrop lands on the dialog itself, not on what's in it.
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });
  return dialog;
}

function openSearch() {
  searchDialog ||= buildSearch();
  if (searchDialog.open) return;
  setMenu(false);
  loadIndex();
  searchDialog.showModal();
  const input = searchDialog.querySelector("input");
  input.value = "";
  searchDialog.querySelector(".search-results").replaceChildren();
  input.focus();
}

document.querySelectorAll("[data-search]").forEach((button) => button.addEventListener("click", openSearch));
document.addEventListener("keydown", (event) => {
  const typing = event.target.closest?.("input, textarea, select, [contenteditable]");
  if (event.key === "/" && !typing && !event.ctrlKey && !event.metaKey && !event.altKey) {
    event.preventDefault();
    openSearch();
  }
});
