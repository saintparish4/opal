// The history book. The page holds the book as a list: front matter written
// by hand, and one chapter per release written by
// base/tools/build-history-book.py from CHANGELOG.md. This builds a book from
// copies of that list, sets each chapter on its own spread (or on as many
// single pages as a phone needs), and turns the leaves on a click, the arrow
// and page keys, a swipe, or the wheel. A reader who asks for reduced motion
// keeps the list, and so does one without JavaScript.
(() => {
  const root = document.documentElement;
  const source = document.getElementById("history");
  const stage = document.getElementById("stage");
  const status = stage.querySelector(".book-status");
  const nextButton = stage.querySelector(".turn-next");
  const motion = matchMedia("(prefers-reduced-motion: no-preference)");
  const narrow = matchMedia("(max-width: 820px)");
  // Type is shrunk a step at a time, only as far as a long chapter needs to
  // fit its spread.
  const FITS = [1, 0.96, 0.92, 0.88, 0.85, 0.82, 0.79];
  const TURN_MS = 900;
  const HURRIED_MS = 420;

  let book = null;

  const el = (tag, className, parent) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (parent) parent.append(node);
    return node;
  };
  const roman = (n) => {
    let out = "";
    for (const [value, letters] of [[1000, "M"], [900, "CM"], [500, "D"], [400, "CD"], [100, "C"], [90, "XC"],
      [50, "L"], [40, "XL"], [10, "X"], [9, "IX"], [5, "V"], [4, "IV"], [1, "I"]]) {
      while (n >= value) { out += letters; n -= value; }
    }
    return out;
  };
  // The list keeps its ids; the book's copies must not repeat them.
  const copy = (node) => {
    const clone = node.cloneNode(true);
    if (clone.nodeType === 1) {
      clone.removeAttribute("id");
      clone.querySelectorAll("[id]").forEach((child) => child.removeAttribute("id"));
    }
    return clone;
  };
  const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2);

  function geometry() {
    const single = narrow.matches;
    const width = root.clientWidth;
    // The book, the line under it, and the page's own heading should all fit
    // in the window together, under the sticky header.
    // A phone's innerHeight can count the space under its toolbars, so take
    // the smaller of it and the layout viewport.
    const tall = Math.min(window.innerHeight, root.clientHeight);
    const room = tall - (stage.getBoundingClientRect().top + window.scrollY) - (single ? 76 : 78);
    let pageW, pageH;
    if (single) {
      pageW = Math.min(width - 32, 520);
      pageH = Math.min(Math.max(room, 480), pageW * 1.75);
    } else {
      pageH = Math.min(Math.max(room, 540), 880);
      pageW = Math.min(pageH * 0.76, (Math.min(width, 1480) - 200) / 2);
      pageH = Math.min(pageH, pageW / 0.6);
    }
    pageW = Math.floor(pageW);
    pageH = Math.floor(pageH);
    const fs = single ? Math.min(17, Math.max(14.5, pageW * 0.043)) : Math.min(16.5, Math.max(12.5, pageW * 0.0292));
    return { mode: single ? "single" : "spread", pageW, pageH, fs };
  }

  function dress(node, geo) {
    node.dataset.mode = geo.mode;
    node.style.setProperty("--page-w", `${geo.pageW}px`);
    node.style.setProperty("--page-h", `${geo.pageH}px`);
    node.style.setProperty("--fs", `${geo.fs}px`);
  }

  // ---- setting the pages ----

  function paginate(meter, mode) {
    const room = el("div", "face", meter);
    const pages = [];
    const fits = (inner) => inner.scrollHeight <= inner.clientHeight + 1;
    const side = () => (mode === "spread" && pages.length % 2 === 1 ? "back" : "front");
    function shell(kind, faceSide) {
      room.className = `face ${faceSide}`;
      const page = el("div", `page ${kind}`);
      const inner = el("div", "page-inner", page);
      room.replaceChildren(page);
      return { page, inner };
    }
    function fitType(page, inner) {
      for (const fit of FITS) {
        page.style.setProperty("--fit", fit);
        if (fits(inner)) return;
      }
    }

    function staticPage(node) {
      const kind = [...node.classList].filter((name) => name !== "sheet").join(" ");
      const { page, inner } = shell(kind, side());
      node.childNodes.forEach((child) => inner.append(copy(child)));
      fitType(page, inner);
      page.remove();
      page.dataset.key = node.dataset.face;
      return { page, key: node.dataset.face, label: node.dataset.label || node.querySelector("h2")?.textContent || "" };
    }

    function units(chapter) {
      const found = [];
      for (const node of chapter.querySelector(".chapter-text").children) {
        if (node.tagName === "UL") for (const item of node.children) found.push({ kind: "li", node: item });
        else found.push({ kind: node.tagName.toLowerCase(), node });
      }
      return found;
    }
    function put(flow, list) {
      let ul = null;
      for (const unit of list) {
        if (unit.kind === "li") {
          ul ||= el("ul", "", flow);
          ul.append(copy(unit.node));
        } else {
          ul = null;
          flow.append(copy(unit.node));
        }
      }
    }
    function opening(inner, chapter, n) {
      const head = copy(chapter.querySelector(".chapter-head"));
      head.style.counterSet = `chapter ${n}`;
      const plate = copy(chapter.querySelector(".plate"));
      const frame = el("div", "plate-box");
      frame.dataset.plate = roman(n);
      frame.append(...plate.childNodes);
      plate.append(frame);
      inner.append(head, plate);
    }
    function tail(inner) {
      const flow = inner.querySelector(".flow") || el("div", "flow chapter-text", inner);
      const mark = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      mark.setAttribute("class", "tail");
      mark.setAttribute("viewBox", "0 0 120 60");
      mark.setAttribute("aria-hidden", "true");
      mark.innerHTML = '<use href="#tailpiece"/>';
      flow.append(mark);
      if (!fits(inner)) mark.remove();
    }

    // A spread: the chapter's heading and plate on the left, the text on the
    // right. A chapter too long for one page starts under its plate. The
    // plate is kept large if a slightly smaller type allows it; failing that
    // it gives up room down to its least size, and only then does the type
    // shrink further.
    function spread(chapter, n) {
      const list = units(chapter);
      const attempt = (fit, k) => {
        const right = shell("chapter", "front");
        right.page.style.setProperty("--fit", fit);
        put(el("div", "flow chapter-text", right.inner), list.slice(k));
        if (!fits(right.inner)) return null;
        right.page.remove();
        const left = shell("chapter opening", "back");
        left.page.style.setProperty("--fit", fit);
        opening(left.inner, chapter, n);
        put(el("div", "flow chapter-text", left.inner), list.slice(0, k));
        const ok = fits(left.inner);
        const share = left.inner.querySelector(".plate").offsetHeight / left.inner.clientHeight;
        left.page.remove();
        return { left, right, ok, share };
      };
      // The fewest units on the left that leave the rest fitting on the right.
      const layout = (fit) => {
        for (let k = 0; k <= list.length; k++) {
          // A heading never ends a page.
          if (k > 0 && k < list.length && list[k - 1].kind === "h3") continue;
          const made = attempt(fit, k);
          if (made) return made;
        }
        return null;
      };
      // Best of all, the whole text faces the plate; next, the text starts
      // under a plate still large.
      let chosen = null;
      for (const fit of FITS.filter((f) => f >= 0.92)) {
        const made = attempt(fit, 0);
        if (made && made.ok) { chosen = made; break; }
      }
      for (const fit of FITS.filter((f) => f >= 0.9)) {
        if (chosen) break;
        const made = layout(fit);
        if (made && made.ok && made.share >= 0.4) chosen = made;
      }
      if (!chosen) {
        for (const fit of FITS) {
          const made = layout(fit);
          if (made && made.ok) { chosen = made; break; }
          chosen = made || chosen;
        }
      }
      if (!chosen.ok) {
        // Nothing fits even at the smallest type: let the pages scroll rather
        // than lose text.
        chosen.left.page.classList.add("overfull");
        chosen.right.page.classList.add("overfull");
      }
      // With text under it, the plate takes only the height its drawing
      // needs, so the page's spare room falls at its foot.
      room.replaceChildren(chosen.left.page);
      const plate = chosen.left.inner.querySelector(".plate");
      if (chosen.left.inner.querySelector(".flow").children.length) {
        const need = plate.offsetWidth * 0.75 + plate.querySelector(".plate-box").offsetHeight - plate.querySelector("svg").getBoundingClientRect().height;
        if (need < plate.offsetHeight) plate.style.flex = `0 0 ${Math.ceil(need)}px`;
      }
      room.replaceChildren(chosen.right.page);
      tail(chosen.right.inner);
      chosen.right.page.remove();
      chosen.left.page.remove();
      return [chosen.left.page, chosen.right.page];
    }

    // On a phone: the heading and plate open the chapter, and the text runs
    // on over as many pages as it takes.
    function singles(chapter, n) {
      const list = units(chapter);
      const out = [];
      let at = 0;
      while (at < list.length || !out.length) {
        const first = !out.length;
        const { page, inner } = shell(first ? "chapter opening" : "chapter", "front");
        if (first) {
          opening(inner, chapter, n);
          // The plate at the height its drawing needs; the text follows.
          const plate = inner.querySelector(".plate");
          const caption = plate.querySelector(".plate-box").offsetHeight - plate.querySelector("svg").getBoundingClientRect().height;
          plate.style.flex = `0 0 ${Math.ceil(plate.offsetWidth * 0.75 + caption)}px`;
        }
        const flow = el("div", "flow chapter-text", inner);
        let end = at;
        while (end < list.length) {
          flow.replaceChildren();
          put(flow, list.slice(at, end + 1));
          if (!fits(inner)) break;
          end++;
        }
        if (end < list.length && end > at + 1 && list[end - 1].kind === "h3") end--;
        if (end === at && !first) {
          end = at + 1;
          page.classList.add("overfull");
        }
        flow.replaceChildren();
        put(flow, list.slice(at, end));
        if (end === list.length) tail(inner);
        page.remove();
        out.push(page);
        at = end;
      }
      return out;
    }

    for (const node of source.children) {
      if (node.matches("[data-face]")) {
        if (mode === "single" && node.hasAttribute("data-spread-only")) continue;
        pages.push(staticPage(node));
      } else if (node.matches(".chapters")) {
        [...node.children].forEach((chapter, at) => {
          // A chapter opens on a left-hand page.
          if (mode === "spread" && pages.length % 2 === 0) pages.push({ page: el("div", "page blank"), key: "blank" });
          const n = at + 1;
          const version = chapter.dataset.version;
                    const made = mode === "spread" ? spread(chapter, n) : singles(chapter, n);
          made.forEach((page, part) => {
            if (part > 0) {
              const head = el("div", "runhead");
              head.textContent = `Chapter ${roman(n)} · ${version}`;
              page.append(head);
            }
            page.dataset.key = chapter.id;
            pages.push({ page, key: chapter.id, part, chapter: n, label: `Chapter ${roman(n)} · ${version}` });
          });
        });
      }
    }
    if (mode === "spread" && pages.length % 2) pages.push({ page: el("div", "page blank"), key: "blank" });
    room.remove();
    return pages;
  }

  // Folios: small roman for the preface and contents, then 1, 2, 3 from the
  // first chapter on. Covers, endpapers, and the title pages go unnumbered,
  // as in a printed book.
  function number(pages) {
    let front = 0;
    let body = 0;
    for (const entry of pages) {
      if (entry.chapter || entry.key === "finis") entry.folio = String(++body);
      else if (!/cover|endpaper|blank/.test(entry.key)) {
        front++;
        if (entry.key === "preface" || entry.key === "contents") entry.folio = roman(front).toLowerCase();
      }
      if (entry.folio) el("div", "folio", entry.page).textContent = entry.folio;
    }
    const firstPage = {};
    for (const entry of pages) if (entry.chapter && !(entry.key in firstPage)) firstPage[entry.key] = entry.folio;
    for (const entry of pages) {
      entry.page.querySelectorAll('.contents a[href^="#v"]').forEach((link) => {
        const folio = el("span", "toc-page", link);
        folio.textContent = firstPage[link.getAttribute("href").slice(1)] || "";
      });
    }
  }

  // ---- the book ----

  function build(keep) {
    // The stage has to be laid out for the book's size to be worked out
    // from where it starts.
    stage.hidden = false;
    const geo = geometry();
    const node = el("div", "book at-rest");
    dress(node, geo);
    node.setAttribute("role", "region");
    node.setAttribute("aria-roledescription", "book");
    node.setAttribute("aria-label", "The history of Opal");
    const block = el("div", "book-block", node);
    const underL = el("div", "under under-l", block);
    const underR = el("div", "under under-r", block);

    const meter = el("div", "book meter");
    dress(meter, geo);
    stage.append(meter);
    const pages = paginate(meter, geo.mode);
    meter.remove();
    number(pages);

    pages.forEach((entry, at) => entry.page.style.setProperty("--mottle-at", `${(at * 211) % 600}px ${(at * 347) % 600}px`));
    const leaves = [];
    const face = (entry, side) => {
      const f = el("div", `face ${side}`);
      f.append(entry ? entry.page : el("div", "page blank"));
      el("div", "shade", f);
      return f;
    };
    const pairs = geo.mode === "spread" ? pages.length / 2 : pages.length;
    for (let j = 0; j < pairs; j++) {
      const front = geo.mode === "spread" ? pages[2 * j] : pages[j];
      const back = geo.mode === "spread" ? pages[2 * j + 1] : null;
      const leaf = el("div", "leaf", block);
      const f = face(front, "front");
      const b = face(back, "back");
      leaf.append(f, b);
      leaves.push({ el: leaf, front: f, back: b, fronts: front, backs: back });
    }
    // Each shadow is clipped to its own half, so it never falls off the book.
    const clipR = el("div", "cast-clip cast-clip-r", block);
    const clipL = el("div", "cast-clip cast-clip-l", block);
    const castR = el("div", "cast cast-r", clipR);
    const castL = el("div", "cast cast-l", clipL);

    book = { el: node, block, underL, underR, castR, castL, leaves, pages, geo, at: 0, queue: 0, busy: false };
    stage.insertBefore(node, nextButton);
    if (keep) book.at = position(keep);
    else if (location.hash) book.at = position({ key: decodeURIComponent(location.hash.slice(1)) });
    settle();
    wire();
    source.classList.add("is-source");
    stage.hidden = false;
    root.classList.remove("book-pending");
  }

  function teardown() {
    if (!book) return;
    book.el.remove();
    book = null;
    source.classList.remove("is-source");
    stage.hidden = true;
  }

  // Where to open after a rebuild or from a link: the leaf that shows the
  // page with this key (and part, for a chapter run over several pages).
  function position({ key, part = 0 }) {
    let found = book.pages.findIndex((entry) => entry.key === key && (entry.part || 0) === part);
    if (found < 0) found = book.pages.findIndex((entry) => entry.key === key);
    if (found < 0) return 0;
    return book.geo.mode === "spread" ? Math.ceil(found / 2) : found;
  }

  function visiblePages() {
    const { pages, at, geo } = book;
    if (geo.mode === "single") return [pages[at]];
    return [pages[2 * at - 1], pages[2 * at]].filter(Boolean);
  }

  const last = () => (book.geo.mode === "spread" ? book.leaves.length : book.leaves.length - 1);

  function settle() {
    const { leaves, at, geo } = book;
    const count = leaves.length;
    const spread = geo.mode === "spread";
    leaves.forEach((leaf, j) => {
      const turned = j < at;
      leaf.el.classList.toggle("turned", turned);
      leaf.el.classList.remove("lift");
      leaf.el.style.transform = "";
      leaf.el.style.zIndex = turned ? j + 1 : count - j;
      // Only the pages that show, or that a turn is about to show, are
      // painted.
      const near = spread ? j >= at - 2 && j <= at + 1 : j === at || j === at + 1;
      leaf.el.classList.toggle("away", !near);
      leaf.front.inert = j !== at;
      leaf.back.inert = !(spread && j === at - 1);
      leaf.el.classList.toggle("can-lift", spread ? (j === at && at < count) || j === at - 1 : j === at && at < count - 1);
      leaf.front.lastChild.style.opacity = 0;
      leaf.back.lastChild.style.opacity = 0;
    });
    book.castR.style.opacity = 0;
    book.castL.style.opacity = 0;
    book.el.dataset.closed = spread && at === 0 ? "front" : spread && at === count ? "back" : "";
    book.underL.hidden = !spread || at === 0;
    book.underR.hidden = spread && at === count;
    const thickness = (n) => `${Math.min(9, n * 0.75).toFixed(2)}px`;
    book.el.style.setProperty("--thick-l", thickness(at));
    book.el.style.setProperty("--thick-r", thickness((spread ? count : count - 1) - at));
    book.el.classList.add("at-rest");
    describe();
  }

  function describe() {
    const shown = visiblePages();
    const names = [];
    for (const entry of shown) {
      const name = entry.label || "";
      if (name && !names.includes(name)) names.push(name);
    }
    const folios = shown.map((entry) => entry.folio).filter(Boolean);
    const where = folios.length > 1 ? `pages ${folios[0]}–${folios[folios.length - 1]}` : folios.length ? `page ${folios[0]}` : "";
    status.textContent = [...names, where].filter(Boolean).join(" · ");
    stage.querySelector(".turn-prev").disabled = book.at === 0;
    nextButton.disabled = book.at >= last();
    const chapter = shown.find((entry) => entry.chapter);
    const hash = chapter ? `#${chapter.key}` : "";
    if (location.hash !== hash) history.replaceState(null, "", hash || location.pathname + location.search);
  }

  // ---- turning ----

  function turn(dir) {
    if (!book) return;
    book.queue += dir;
    if (!book.busy) step();
  }

  function step() {
    if (!book || !book.queue) return;
    const dir = Math.sign(book.queue);
    if ((dir > 0 && book.at >= last()) || (dir < 0 && book.at <= 0)) {
      book.queue = 0;
      return;
    }
    book.queue -= dir;
    book.busy = true;
    const leaf = book.leaves[dir > 0 ? book.at : book.at - 1];
    animate(leaf, dir, book.queue ? HURRIED_MS : TURN_MS).then(() => {
      if (!book) return;
      book.at += dir;
      book.busy = false;
      settle();
      step();
    });
  }

  // Straight to a page far off: the book is opened one leaf short of it,
  // unseen, and only the last leaf turns.
  function go(target) {
    if (!book || book.busy) return;
    target = Math.max(0, Math.min(last(), target));
    if (target === book.at) return;
    const dir = Math.sign(target - book.at);
    if (Math.abs(target - book.at) > 1) {
      book.at = target - dir;
      settle();
    }
    turn(dir);
  }

  // A rigid leaf rotating about the spine. As it lifts, its face darkens and
  // it throws a shadow on the page beneath, which runs ahead of its free
  // edge; past upright, the same on the other side as it comes down.
  function animate(leaf, dir, ms) {
    const { castR, castL, geo, leaves } = book;
    const spread = geo.mode === "spread";
    leaf.el.classList.remove("away", "lift");
    leaf.el.style.zIndex = 1000;
    book.el.classList.remove("at-rest");
    // Opening or closing a cover leaves one half with nothing under the
    // turning leaf.
    const next = book.at + dir;
    book.underL.hidden = !spread || Math.min(book.at, next) === 0;
    book.underR.hidden = spread && Math.max(book.at, next) === leaves.length;
    if (spread) book.el.dataset.closed = next === 0 ? "front" : next === leaves.length ? "back" : "";
    const frontShade = leaf.front.lastChild;
    const backShade = leaf.back.lastChild;
    return new Promise((done) => {
      const start = performance.now();
      const frame = (now) => {
        if (!book) return done();
        const t = Math.min(1, (now - start) / ms);
        const p = ease(t);
        const angle = 180 * (dir > 0 ? p : 1 - p);
        const rad = (angle * Math.PI) / 180;
        const lift = Math.sin(rad);
        leaf.el.style.transform = `rotateY(${-angle}deg)`;
        frontShade.style.opacity = angle < 90 ? (angle / 90) * 0.85 : 0;
        backShade.style.opacity = angle > 90 ? ((180 - angle) / 90) * 0.85 : 0;
        // Where the free edge appears: nearer the reader as it lifts, so
        // pushed outward by the perspective (7 page widths, as in the CSS).
        const edge = ((Math.cos(rad) * 7) / (7 - lift)) * 100;
        if (angle < 90) {
          castR.style.opacity = (lift * 0.95).toFixed(3);
          castR.style.transform = `translateX(${edge}%) scaleX(${(0.15 + 0.85 * lift).toFixed(3)})`;
          castL.style.opacity = 0;
        } else {
          castL.style.opacity = spread ? (lift * 0.95).toFixed(3) : 0;
          castL.style.transform = `translateX(${edge}%) scaleX(${(0.15 + 0.85 * lift).toFixed(3)})`;
          castR.style.opacity = 0;
        }
        if (t < 1) requestAnimationFrame(frame);
        else done();
      };
      requestAnimationFrame(frame);
    });
  }

  // ---- input ----

  let wired = false;
  let wheelSum = 0;
  let wheelQuietUntil = 0;
  let lastWheelTurn = 0;
  let swipe = null;
  let swiped = 0;

  function wire() {
    const { block, el: node } = book;
    block.addEventListener("click", (event) => {
      if (performance.now() - swiped < 400) return;
      if (event.target.closest("a, button")) return;
      if (!getSelection().isCollapsed) return;
      const face = event.target.closest(".face");
      if (!face) return;
      if (book.geo.mode === "spread") turn(face.classList.contains("back") ? -1 : 1);
      else {
        const box = node.getBoundingClientRect();
        turn(event.clientX - box.left < box.width * 0.3 ? -1 : 1);
      }
    });
    block.addEventListener("click", (event) => {
      const link = event.target.closest('a[href^="#v"]');
      if (!link) return;
      event.preventDefault();
      go(position({ key: link.getAttribute("href").slice(1) }));
    });
    block.addEventListener("pointerover", (event) => {
      const leaf = event.target.closest(".leaf");
      if (leaf && leaf.classList.contains("can-lift") && event.pointerType === "mouse") leaf.classList.add("lift");
    });
    block.addEventListener("pointerout", (event) => {
      const leaf = event.target.closest(".leaf");
      if (leaf && !leaf.contains(event.relatedTarget)) leaf.classList.remove("lift");
    });
    // The wheel turns one leaf per gesture: a trackpad's coasting tail, or a
    // fast spin of a wheel, would otherwise run through the whole book. A
    // gesture that keeps going turns another leaf now and then. At either
    // end, a wheel toward the outside scrolls the page as usual.
    node.addEventListener("wheel", (event) => {
      if (event.ctrlKey) return;
      const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
      const dir = Math.sign(delta);
      if (!dir || (dir > 0 && book.at + book.queue >= last()) || (dir < 0 && book.at + book.queue <= 0)) return;
      event.preventDefault();
      const now = performance.now();
      const amount = event.deltaMode === 1 ? delta * 16 : event.deltaMode === 2 ? delta * 400 : delta;
      const gesture = now < wheelQuietUntil;
      wheelQuietUntil = now + 180;
      if (gesture && !(now - lastWheelTurn > 850 && Math.abs(amount) >= 4)) return;
      wheelSum = gesture ? amount : wheelSum + amount;
      if (Math.abs(wheelSum) >= 24 || gesture) {
        turn(dir);
        lastWheelTurn = now;
        wheelSum = 0;
      }
    }, { passive: false });
    node.addEventListener("pointerdown", (event) => {
      if (event.pointerType !== "mouse") swipe = { x: event.clientX, y: event.clientY };
    });
    node.addEventListener("pointerup", (event) => {
      if (!swipe) return;
      const dx = event.clientX - swipe.x;
      const dy = event.clientY - swipe.y;
      swipe = null;
      if (Math.abs(dx) > 40 && Math.abs(dx) > Math.abs(dy) * 1.5) {
        swiped = performance.now();
        turn(dx < 0 ? 1 : -1);
      }
    });
    node.addEventListener("pointercancel", () => { swipe = null; });

    if (wired) return;
    wired = true;
    stage.querySelectorAll("[data-turn]").forEach((button) =>
      button.addEventListener("click", () => turn(Number(button.dataset.turn))));
    document.addEventListener("keydown", (event) => {
      if (!book || event.altKey || event.ctrlKey || event.metaKey) return;
      if (event.target.closest?.("input, textarea, select, [contenteditable]") || document.querySelector("dialog[open]")) return;
      const keys = { ArrowRight: 1, PageDown: 1, ArrowLeft: -1, PageUp: -1 };
      if (event.key in keys) {
        event.preventDefault();
        turn(keys[event.key]);
      } else if (event.key === "Home" || event.key === "End") {
        event.preventDefault();
        go(event.key === "Home" ? 0 : last());
      }
    });
    window.addEventListener("hashchange", () => {
      if (book && location.hash) go(position({ key: decodeURIComponent(location.hash.slice(1)) }));
    });
  }

  // ---- when to build ----

  let pendingResize = 0;
  function rebuild() {
    if (!book) return start();
    const shown = visiblePages();
    const anchor = shown.find((entry) => entry.chapter) || shown[shown.length - 1] || { key: "cover" };
    const keep = { key: anchor.key, part: book.geo.mode === "single" ? anchor.part || 0 : 0 };
    teardown();
    build(keep);
  }
  function start() {
    if (motion.matches) build();
    else root.classList.remove("book-pending");
  }

  // Pages are measured in the book's own type, so wait for it; a slow or
  // blocked font gives up after a moment and the book is set in the
  // fallback serif, then set again if the font arrives.
  const faces = ['400 1em "EB Garamond"', 'italic 400 1em "EB Garamond"', '600 1em "EB Garamond"',
    '1em "IM Fell English"', 'italic 1em "IM Fell English"', '1em "IM Fell English SC"', '1em "Courier Prime"'];
  Promise.race([
    Promise.all(faces.map((font) => document.fonts.load(font))),
    new Promise((resolve) => setTimeout(resolve, 2500)),
  ]).then(() => {
    start();
    let late = 0;
    document.fonts.addEventListener("loadingdone", () => {
      clearTimeout(late);
      late = setTimeout(() => book && !book.busy && rebuild(), 120);
    });
  });

  motion.addEventListener("change", () => (motion.matches ? build() : teardown()));
  window.addEventListener("resize", () => {
    clearTimeout(pendingResize);
    pendingResize = setTimeout(() => {
      if (!book || book.busy) return;
      const geo = geometry();
      if (geo.mode !== book.geo.mode || geo.pageW !== book.geo.pageW || geo.pageH !== book.geo.pageH) rebuild();
    }, 180);
  });
})();
