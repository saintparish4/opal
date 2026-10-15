import { defineConfig } from "astro/config";
import { satteri } from "@astrojs/markdown-satteri";
import searchIndex from "./src/integrations/search-index.mjs";

export default defineConfig({
  site: "https://opalx.dev",
  build: { format: "directory" },
  // The pages were written by hand with their whitespace meaning something
  // (inline elements separated by a newline, the terminal demo's spacing), so
  // they're emitted as written.
  compressHTML: false,
  integrations: [searchIndex()],
  markdown: {
    // `## What's new {#added}`: posts keep the anchors they were published
    // with, which the slugger would otherwise rename.
    // Smart punctuation is off because it also rewrites link text, turning a
    // compare URL's `...` into `…`.
    processor: satteri({ features: { headingAttributes: true, smartPunctuation: false } }),
    syntaxHighlight: false,
  },
});
