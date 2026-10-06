// Medians from the 2026-10-05 section of benchmarks/BENCHMARKS.md (laptop, a
// build of the v0.4.0 source made before the release). Times in seconds; DISK
// is megabytes. Keep in step with that file.
const TOOLS = [
  { name: "opal", version: "v0.4.0 pre-release" },
  { name: "npm", version: "v12.0.2" },
  { name: "pnpm", version: "v11.21.0" },
  { name: "yarn", version: "v1.22.22" },
  { name: "bun", version: "v1.4.2" },
];
const BENCH = {
  next: {
    cold: [29.34, 26.37, 21.51, 59.26, 20.02],
    ci: [22.32, 12.79, 17.72, 53.13, 14.46],
    warm: [0.874, 10.8, 2.23, 5.48, 1.08],
    cached: [1.08, 10.66, 2.15, 5.16, 1.12],
    noop: [0.072, 0.625, 0.465, 0.349, 0.016],
    add: [0.847, 0.82, 3.72, 2.39, 0.163],
  },
  express: {
    cold: [0.964, 1.6, 1.32, 1.6, 0.617],
    ci: [0.546, 0.906, 1.03, 1.16, 0.733],
    warm: [0.103, 0.6, 0.732, 0.528, 0.062],
    cached: [0.108, 0.608, 0.716, 0.519, 0.067],
    noop: [0.026, 0.347, 0.449, 0.256, 0.006],
    add: [0.572, 0.517, 0.848, 0.563, 0.145],
  },
};
// What a second copy of the same project adds on disk, from the same cache.
const DISK = {
  next: [10.5, 463.3, 13.7, 587.1, 8.3],
  express: [0.8, 4.3, 1.1, 4.2, 0.5],
};
// Opal's and npm's ranges overlap in BENCHMARKS.md although the medians are
// more than a tenth apart (Next.js first install: opal 28.19–30.66s, npm
// 24.59–29.09s; express add: opal 190–635ms, npm 517–672ms).
const OVERLAP = new Set(["next cold", "express add"]);
// The hero shows one scenario, a reinstall, for each project. The other three
// are in the grid further down, where Opal's slower ones sit beside it.
const HERO_SCENARIO = "warm";
const HERO_CAPTION = "lockfile + cache, node_modules deleted · seconds (lower is better)";
const HERO = {
  next: { title: "Installing a Next.js app", workload: "Next.js 16.3.2 defaults · about 360 packages" },
  express: { title: "Installing express", workload: "express ^5 · 68 packages" },
};
const GRID = [
  { project: "next", scenario: "warm", title: "Reinstall, Next.js", note: "lockfile + cache · node_modules deleted" },
  { project: "next", scenario: "noop", title: "Nothing to do, Next.js", note: "everything already installed" },
  { project: "express", scenario: "warm", title: "Reinstall, express", note: "lockfile + cache · node_modules deleted" },
  { project: "next", scenario: "cold", title: "First install, Next.js", note: "no lockfile · no cache" },
  { project: "next", scenario: "ci", title: "CI, Next.js", note: "lockfile only · empty cache" },
  { project: "express", scenario: "cold", title: "First install, express", note: "no lockfile · no cache" },
  { project: "next", scenario: "cached", title: "CI with its cache, Next.js", note: "frozen lockfile · cache restored" },
  { project: "next", scenario: "add", title: "Add one package, Next.js", note: "one new dependency · installed project" },
  { project: "next", scenario: "disk", title: "A second copy on disk, Next.js", note: "same project again · megabytes added" },
];

function values(project, scenario) {
  return scenario === "disk" ? DISK[project] : BENCH[project][scenario];
}

// A chart is in seconds unless it is the disk one.
function amount(value, scenario) {
  if (scenario !== "disk") return seconds(value);
  return `${value < 100 ? value : Math.round(value)} MB`;
}

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
  const measured = values(project, scenario);
  const all = TOOLS.map((tool, index) => ({ ...tool, value: measured[index] }));
  return [all[0], ...all.slice(1).sort((a, b) => a.value - b.value)];
}

// The scale under a chart: a round step that puts four to six ticks under the
// longest bar, ending on the first tick at or past it. Bars are drawn against
// that end, not against the longest bar, so a tick means what it says.
function scale(longest) {
  const magnitude = 10 ** Math.floor(Math.log10(longest));
  const step = [0.2, 0.5, 1, 2, 5].map((unit) => unit * magnitude).find((size) => longest / size <= 6);
  const ticks = [];
  for (let at = 0; at < longest + step - 1e-9; at += step) ticks.push(at);
  return { end: ticks[ticks.length - 1], ticks };
}

// One unit for a whole scale, so "500ms" never sits beside "1.00s".
function tickLabel(value, end, scenario) {
  if (scenario === "disk") return `${Math.round(value)}MB`;
  if (end < 1) return `${Math.round(value * 1000)}ms`;
  return `${Number(value.toFixed(1))}s`;
}

function drawBars(list, project, scenario, withVersions) {
  const data = rows(project, scenario);
  const { end, ticks } = scale(Math.max(...data.map((row) => row.value)));
  const axis = element("li", "axis");
  axis.setAttribute("aria-hidden", "true");
  const ruler = element("span", "axis-ticks");
  ticks.forEach((tick) => {
    const mark = element("span", "", tickLabel(tick, end, scenario));
    mark.style.left = `${(tick / end) * 100}%`;
    ruler.append(mark);
  });
  axis.append(element("span"), ruler, element("span"));
  list.replaceChildren(
    ...data.map((row) => {
      const item = element("li", row.name === "opal" ? "is-opal" : "");
      const name = element("span", "bar-name", row.name);
      if (withVersions) name.append(element("small", "", row.version));
      const track = element("span", "bar-track");
      const fill = element("span", "bar-fill");
      // Never thinner than a sliver, so the fastest tool still has a bar.
      fill.style.width = `${Math.max((row.value / end) * 100, 1)}%`;
      track.append(fill);
      item.append(name, track, element("span", "bar-value", amount(row.value, scenario)));
      return item;
    }),
    axis,
  );
}

function againstNpm(project, scenario) {
  const [opal, npm] = values(project, scenario);
  const ratio = opal < npm ? npm / opal : opal / npm;
  const [better, worse] = scenario === "disk" ? ["less", "more"] : ["faster", "slower"];
  // Neither tool is called faster when their ranges overlap. Medians within
  // a tenth of each other always do; OVERLAP names the wider gaps that do too.
  if (ratio < 1.1 || OVERLAP.has(`${project} ${scenario}`)) return { text: "level with npm", faster: false };
  const shown = ratio >= 10 ? Math.round(ratio) : ratio.toFixed(1);
  return { text: `${shown}× ${opal < npm ? better : worse} than npm`, faster: opal < npm };
}

const heroState = { project: "next" };
function drawHero() {
  const { title, workload } = HERO[heroState.project];
  drawBars(document.getElementById("hero-bars"), heroState.project, HERO_SCENARIO, true);
  document.getElementById("hero-title").textContent = title;
  document.getElementById("hero-caption").textContent = HERO_CAPTION;
  document.getElementById("hero-workload").textContent = workload;
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

// The demo's output is in terminal-demo.js, generated from a real terminal
// capture: opal's own lines with its own colours. A step either has lines, or
// (the first install) frames of the lines that redraw while it works and then
// the lines it leaves behind. Pauses are multiples of 40ms, the grain `wait`
// counts in, so the line under a step's number can be timed to end with it.
const DEMO = window.OPAL_DEMO;
const STEPS = [
  { command: "opal install", ...DEMO.install },
  { command: "opal install", lines: DEMO.again },
  { command: "opal install --frozen-lockfile", lines: DEMO.frozen },
  { command: "opal cache verify", lines: DEMO.verify },
  { command: "opal upgrade", lines: DEMO.upgrade },
];
const BEFORE_TYPING = 360;
const PER_LETTER = 40;
const PER_LINE = 160;
const BEFORE_OUTPUT = 280;
const AFTER_STEP = 2600;

// How long a step plays for, from the first keystroke to the next step.
function stepDuration({ command, header, frames = [], result = [], lines = [] }) {
  const output = header
    ? BEFORE_OUTPUT + frames.reduce((sum, frame) => sum + frame.ms, 0) + result.length * PER_LINE
    : BEFORE_OUTPUT + lines.length * PER_LINE;
  return BEFORE_TYPING + command.length * PER_LETTER + output + AFTER_STEP;
}

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
    tab.classList.remove("playing");
  });
  document.getElementById("step-count").textContent =
    `${String(index + 1).padStart(2, "0")} / ${String(STEPS.length).padStart(2, "0")}`;
}

// Starts the line under the step's number, which fills for as long as the
// step plays. Reading a layout property between removing the class and adding
// it is what makes a replay start the animation over.
function startLine(index, continues) {
  const tab = stepTabs[index];
  tab.style.setProperty("--step-ms", `${stepDuration(STEPS[index]) - (continues ? 0 : AFTER_STEP)}ms`);
  void tab.offsetWidth;
  tab.classList.add("playing");
}

// A line of captured output. The markup is opal's colour codes turned into
// spans by the script that built terminal-demo.js, never anything typed in.
function captured(markup, className = "") {
  const line = element("span", className);
  line.innerHTML = markup;
  return line;
}

// Types the command, plays its output, then moves on.
async function playStep(index, thenContinue = true) {
  const run = ++playback;
  current = index;
  markStep(index);
  const { command, header, frames = [], result = [], lines = [] } = STEPS[index];
  const settled = header ? [header, ...result] : lines;
  const out = document.getElementById("terminal-out");
  const typed = element("span", "typed");
  const caret = element("span", "caret");
  out.replaceChildren(element("span", "prompt", "$ "), typed, caret);

  if (stillMotion) {
    typed.textContent = command;
    caret.remove();
    settled.forEach((markup) => out.append("\n", captured(markup)));
    return;
  }

  startLine(index, thenContinue);
  if (!(await wait(BEFORE_TYPING, run))) return;
  for (const letter of command) {
    typed.textContent += letter;
    if (!(await wait(PER_LETTER, run))) return;
  }
  caret.remove();
  if (!(await wait(BEFORE_OUTPUT, run))) return;

  if (header) {
    out.append("\n", captured(header, "line"));
    // The lines a terminal redraws in place: one element, rewritten per frame.
    const live = element("span", "live");
    out.append("\n", live);
    for (const frame of frames) {
      live.innerHTML = frame.html;
      if (!(await wait(frame.ms, run))) return;
    }
    live.previousSibling.remove();
    live.remove();
  }
  for (const markup of header ? result : lines) {
    out.append("\n", captured(markup, "line"));
    if (!(await wait(PER_LINE, run))) return;
  }
  out.append("\n", element("span", "prompt", "$ "), element("span", "caret"));
  if (!thenContinue) return;
  if (!(await wait(AFTER_STEP, run))) return;
  playStep((index + 1) % STEPS.length);
}

// Pausing holds the playback and the step's line together.
function setPaused(value) {
  paused = value;
  terminal.closest(".minute").classList.toggle("is-paused", value);
}
["mouseenter", "focusin"].forEach((name) => terminal.addEventListener(name, () => setPaused(true)));
["mouseleave", "focusout"].forEach((name) => terminal.addEventListener(name, () => setPaused(false)));
document.getElementById("replay").addEventListener("click", () => {
  setPaused(false);
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
    if (tab.dataset.project) {
      heroState.project = tab.dataset.project;
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
      button.title = "Copied";
      button.classList.add("done");
    } catch {
      // No clipboard access: select the command so it can be copied by hand.
      const range = document.createRange();
      range.selectNodeContents(source);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      button.title = "Selected: press Ctrl+C";
    }
    setTimeout(() => {
      button.title = "Copy";
      button.classList.remove("done");
    }, 1800);
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
