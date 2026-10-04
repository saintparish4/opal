// Medians from the preliminary section of benchmarks/BENCHMARKS.md (laptop,
// 2026-10-03, an unreleased build of master at 2ba79e2). Times in seconds.
// Keep in step with that file.
const TOOLS = [
  { name: "opal", version: "master" },
  { name: "npm", version: "v11.17.0" },
  { name: "pnpm", version: "v11.17.0" },
  { name: "yarn", version: "v1.22.22" },
  { name: "bun", version: "v1.3.14" },
];
const BENCH = {
  next: {
    cold: [30.35, 33.03, 25.18, 57.7, 20.09],
    ci: [24.02, 15.27, 19.19, 48.04, 14.85],
    warm: [0.951, 11.87, 2.47, 5.63, 1.17],
    noop: [0.083, 0.628, 0.519, 0.363, 0.021],
  },
  express: {
    cold: [0.946, 1.69, 1.35, 1.59, 0.605],
    ci: [0.499, 1.01, 1.27, 1.23, 0.327],
    warm: [0.093, 0.648, 0.818, 0.585, 0.078],
    noop: [0.016, 0.357, 0.502, 0.295, 0.011],
  },
};
const CAPTIONS = {
  warm: "lockfile + cache, node_modules deleted · seconds (lower is better)",
  noop: "everything already installed · seconds (lower is better)",
  cold: "no lockfile, no cache · seconds (lower is better)",
  ci: "lockfile only, empty cache · seconds (lower is better)",
};
const GRID = [
  { project: "next", scenario: "warm", title: "Reinstall, Next.js", note: "lockfile + cache · node_modules deleted" },
  { project: "next", scenario: "noop", title: "Nothing to do, Next.js", note: "everything already installed" },
  { project: "express", scenario: "warm", title: "Reinstall, express", note: "lockfile + cache · node_modules deleted" },
  { project: "next", scenario: "cold", title: "First install, Next.js", note: "no lockfile · no cache" },
  { project: "next", scenario: "ci", title: "CI, Next.js", note: "lockfile only · empty cache" },
  { project: "express", scenario: "cold", title: "First install, express", note: "no lockfile · no cache" },
];

function seconds(value) {
  return value < 1 ? `${Math.round(value * 1000)}ms` : `${value.toFixed(2)}s`;
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

// Opal first, then the rest fastest to slowest: the page is about Opal, and
// the comparison reads from its row.
function rows(project, scenario) {
  const values = BENCH[project][scenario];
  const all = TOOLS.map((tool, index) => ({ ...tool, value: values[index] }));
  return [all[0], ...all.slice(1).sort((a, b) => a.value - b.value)];
}

function drawBars(list, project, scenario, withVersions) {
  const data = rows(project, scenario);
  const longest = Math.max(...data.map((row) => row.value));
  list.replaceChildren(
    ...data.map((row) => {
      const item = element("li", row.name === "opal" ? "is-opal" : "");
      const name = element("span", "bar-name", row.name);
      if (withVersions) name.append(element("small", "", row.version));
      const track = element("span", "bar-track");
      const fill = element("span", "bar-fill");
      // Never thinner than a sliver, so the fastest tool still has a bar.
      fill.style.width = `${Math.max((row.value / longest) * 100, 1)}%`;
      track.append(fill);
      item.append(name, track, element("span", "bar-value", seconds(row.value)));
      return item;
    }),
  );
}

function againstNpm(project, scenario) {
  const [opal, npm] = BENCH[project][scenario];
  const ratio = opal < npm ? npm / opal : opal / npm;
  // Medians this close come from ranges that overlap (Next.js first install:
  // opal 29.88–34.51s, npm 32.35–33.29s), so neither tool is called faster.
  if (ratio < 1.1) return { text: "level with npm", faster: false };
  const shown = ratio >= 10 ? Math.round(ratio) : ratio.toFixed(1);
  return { text: `${shown}× ${opal < npm ? "faster" : "slower"} than npm`, faster: opal < npm };
}

const heroState = { scenario: "warm" };
function drawHero() {
  drawBars(document.getElementById("hero-bars"), "next", heroState.scenario, true);
  document.getElementById("hero-caption").textContent = CAPTIONS[heroState.scenario];
}

function drawGrid() {
  document.getElementById("grid-charts").replaceChildren(
    ...GRID.map(({ project, scenario, title, note }) => {
      const card = element("article", "mini");
      card.append(element("h3", "", title), element("p", "mini-note", note));
      const list = element("ol", "bars compact");
      drawBars(list, project, scenario, false);
      const verdict = againstNpm(project, scenario);
      card.append(list, element("p", verdict.faster ? "verdict win" : "verdict", verdict.text));
      return card;
    }),
  );
}

// Real output, captured 2026-10-04 from an unreleased build of master at
// efe3d07 on a 364-package Next.js app, with stderr not a terminal, which is
// where the one-line-per-stage output comes from. The charts show master too,
// and v0.3.1's two-minute first install beside them read as a contradiction.
// Warnings are left out, and `opal upgrade` is v0.3.1's line: it reports the
// released version, which hasn't changed. Each entry is [text, pause before
// it in ms, "done" for the line that reports the result].
const STEPS = [
  { command: "opal install", lines: [
    ["Resolving dependencies", 350],
    ["Installing 364 packages", 900],
    ["Linking 364 packages", 1100],
    ["364 packages installed in 25.9s  (resolve 5.3s, fetch 19.6s, link 942ms)", 500, "done"],
    ["skipped 66 optional packages built for other platforms", 120],
  ] },
  { command: "opal install", lines: [
    ["Installing 364 packages", 250],
    ["Linking 364 packages", 200],
    ["364 packages already installed (518ms)", 250, "done"],
    ["skipped 66 optional packages built for other platforms", 120],
  ] },
  { command: "opal install --frozen-lockfile", lines: [
    ["Installing 364 packages", 250],
    ["Linking 364 packages", 200],
    ["364 packages already installed (522ms)", 250, "done"],
    ["skipped 66 optional packages built for other platforms", 120],
  ] },
  { command: "opal cache verify", lines: [["all objects match their hash keys", 1100, "done"]] },
  { command: "opal upgrade", lines: [["opal 0.3.1 is already installed", 700, "done"]] },
];

const stepTabs = [...document.querySelectorAll("[data-step]")];
const terminal = document.querySelector(".terminal");
const stillMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
// Bumped whenever a new playback starts, so an older one stops at its next wait.
let playback = 0;
let paused = false;
let current = 0;

function wait(ms, run) {
  return new Promise((resolve) => {
    const tick = (left) => {
      if (run !== playback) return resolve(false);
      if (paused) return setTimeout(() => tick(left), 120);
      if (left <= 0) return resolve(true);
      setTimeout(() => tick(left - 40), 40);
    };
    tick(ms);
  });
}

function markStep(index) {
  stepTabs.forEach((tab, at) => {
    tab.setAttribute("aria-selected", String(at === index));
    tab.tabIndex = at === index ? 0 : -1;
  });
  document.getElementById("step-count").textContent =
    `${String(index + 1).padStart(2, "0")} / ${String(STEPS.length).padStart(2, "0")}`;
}

// A result line leads with its package count in bold; every other line is dim.
function outputLine(text, kind) {
  if (kind !== "done") return element("span", "dim", text);
  const line = element("span", "done");
  const count = text.match(/^\d+ packages/);
  if (!count) {
    line.textContent = text;
    return line;
  }
  line.append(element("b", "", count[0]), text.slice(count[0].length));
  return line;
}

// Types the command, plays its output a line at a time, then moves on.
async function playStep(index, thenContinue = true) {
  const run = ++playback;
  current = index;
  markStep(index);
  const { command, lines } = STEPS[index];
  const out = document.getElementById("terminal-out");
  const typed = element("span", "typed");
  const caret = element("span", "caret");
  out.replaceChildren(element("span", "prompt", "$ "), typed, caret);

  if (stillMotion) {
    typed.textContent = command;
    caret.remove();
    lines.forEach(([text, , kind]) => out.append("\n", outputLine(text, kind)));
    return;
  }

  if (!(await wait(350, run))) return;
  for (const letter of command) {
    typed.textContent += letter;
    if (!(await wait(38, run))) return;
  }
  caret.remove();
  for (const [text, pause, kind] of lines) {
    if (!(await wait(pause, run))) return;
    const line = outputLine(text, kind);
    line.classList.add("line");
    out.append("\n", line);
  }
  out.append("\n", element("span", "prompt", "$ "), element("span", "caret"));
  if (!thenContinue) return;
  if (!(await wait(2600, run))) return;
  playStep((index + 1) % STEPS.length);
}

["mouseenter", "focusin"].forEach((name) => terminal.addEventListener(name, () => (paused = true)));
["mouseleave", "focusout"].forEach((name) => terminal.addEventListener(name, () => (paused = false)));
document.getElementById("replay").addEventListener("click", () => {
  paused = false;
  playStep(current);
});

// One handler for every tab list: arrow keys move, click selects.
document.querySelectorAll('[role="tablist"]').forEach((tablist) => {
  const tabs = [...tablist.querySelectorAll('[role="tab"]')];
  const vertical = tablist.getAttribute("aria-orientation") === "vertical";

  function select(tab) {
    tabs.forEach((other) => {
      const chosen = other === tab;
      other.setAttribute("aria-selected", String(chosen));
      other.tabIndex = chosen ? 0 : -1;
      const panel = other.getAttribute("aria-controls");
      if (panel) document.getElementById(panel).hidden = !chosen;
    });
    if (tab.dataset.scenario) {
      heroState.scenario = tab.dataset.scenario;
      drawHero();
    }
    if (tab.dataset.step) playStep(Number(tab.dataset.step));
  }

  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => select(tab));
    tab.addEventListener("keydown", (event) => {
      const keys = vertical ? { ArrowDown: 1, ArrowUp: -1 } : { ArrowRight: 1, ArrowLeft: -1 };
      const step = keys[event.key];
      if (!step) return;
      event.preventDefault();
      const next = tabs[(index + step + tabs.length) % tabs.length];
      next.focus();
      select(next);
    });
  });
});

document.querySelectorAll(".copy").forEach((button) => {
  button.addEventListener("click", async () => {
    const source = document.getElementById(button.dataset.copy);
    try {
      await navigator.clipboard.writeText(source.textContent.trim());
      button.textContent = "Copied";
    } catch {
      // No clipboard access: select the command so it can be copied by hand.
      const range = document.createRange();
      range.selectNodeContents(source);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = "Selected";
    }
    setTimeout(() => (button.textContent = "Copy"), 1800);
  });
});

drawHero();
drawGrid();
// Start when the terminal scrolls into view, so the first step isn't played
// to nobody.
if (stillMotion || !("IntersectionObserver" in window)) {
  playStep(0);
} else {
  markStep(0);
  const seen = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) {
      seen.disconnect();
      playStep(0);
    }
  }, { threshold: 0.4 });
  seen.observe(terminal);
}
