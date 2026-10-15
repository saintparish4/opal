import { readFile, readdir, writeFile } from "node:fs/promises";

// What the header's search looks through: the home page's sections and the
// blog's posts. Read from the built pages, so it can't drift from them.

const named = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };

function decode(text) {
  return text.replace(/&(#x[0-9a-f]+|#[0-9]+|[a-z]+);/gi, (entity, name) => {
    if (name[0] === "#") {
      const code = name[1] === "x" || name[1] === "X" ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return String.fromCodePoint(code);
    }
    return named[name.toLowerCase()] ?? entity;
  });
}

function textOf(fragment) {
  const bare = fragment
    .replace(/<(script|style|svg)\b[\s\S]*?<\/\1>/g, " ")
    .replace(/<br\s*\/?>/g, " ")
    .replace(/<[^>]+>/g, " ");
  return decode(bare).replace(/\s+/g, " ").trim();
}

const clip = (text, length) => Array.from(text).slice(0, length).join("");

const HOME_TITLES = { top: "Install Opal", "install-bench": "Benchmarks: ", limits: "Limits: " };

async function entriesFrom(dist) {
  const entries = [];
  const home = await readFile(new URL("index.html", dist), "utf8");
  for (const [, anchor, body] of home.matchAll(/<section class="[^"]*" id="([a-z-]+)">([\s\S]*?)<\/section>/g)) {
    const heading = body.match(/<h[12][^>]*>([\s\S]*?)<\/h[12]>/);
    let title = heading ? textOf(heading[1]) : anchor;
    if (anchor === "top") title = HOME_TITLES.top;
    else if (anchor in HOME_TITLES) title = HOME_TITLES[anchor] + title;
    entries.push({ title, url: `#${anchor}`, kind: "Home", text: clip(textOf(body), 1400) });
  }
  const slugs = (await readdir(new URL("blog/", dist))).filter((name) => name.startsWith("opal-v")).sort().reverse();
  for (const slug of slugs) {
    const page = await readFile(new URL(`blog/${slug}/index.html`, dist), "utf8");
    const title = textOf(page.match(/<h1>([\s\S]*?)<\/h1>/)[1]);
    const prose = page.match(/<article class="prose">([\s\S]*?)<\/article>/)[1];
    entries.push({ title, url: `blog/${slug}/`, kind: "Post", text: clip(textOf(prose), 1800) });
    const parts = prose.split(/(<h[23] id="[^"]+">[\s\S]*?<\/h[23]>)/);
    for (let i = 1; i < parts.length; i += 2) {
      const [, id, heading] = parts[i].match(/<h[23] id="([^"]+)">([\s\S]*?)<\/h[23]>/);
      entries.push({
        title: `${title}: ${textOf(heading)}`,
        url: `blog/${slug}/#${id}`,
        kind: "Post",
        text: clip(textOf(parts[i + 1]), 900),
      });
    }
  }
  entries.push({ title: "Blog", url: "blog/", kind: "Page", text: "Release notes and writing about Opal. RSS feed." });
  return entries;
}

export default function searchIndex() {
  return {
    name: "opal-search-index",
    hooks: {
      "astro:build:done": async ({ dir, logger }) => {
        const entries = await entriesFrom(dir);
        await writeFile(
          new URL("search-index.js", dir),
          `window.OPAL_SEARCH = ${JSON.stringify(entries, null, 1)};\n`,
        );
        logger.info(`search-index.js: ${entries.length} entries`);
      },
    },
  };
}
