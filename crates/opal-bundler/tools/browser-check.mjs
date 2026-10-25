// Loads a page in Chromium and reports what a person would notice: console
// errors, page errors, failed requests, and whether an interaction works.
//
//   node browser-check.mjs <url> [--click <selector>] [--expect <text>]
//
// Prints one JSON object and exits 1 if anything went wrong. Uses
// Playwright's Chromium, or Chrome (OPAL_CHROMIUM names an executable).

import { chromium } from "playwright";

const [url, ...rest] = process.argv.slice(2);
const option = (name) => {
  const index = rest.indexOf(name);
  return index === -1 ? undefined : rest[index + 1];
};
const click = option("--click");
const expected = option("--expect");

async function launch() {
  if (process.env.OPAL_CHROMIUM) return chromium.launch({ executablePath: process.env.OPAL_CHROMIUM });
  try {
    return await chromium.launch();
  } catch {
    return chromium.launch({ channel: "chrome" });
  }
}

const browser = await launch();
const problems = [];
try {
  const page = await browser.newPage();
  page.on("console", (message) => {
    if (message.type() === "error") problems.push(`console error: ${message.text()}`);
  });
  page.on("pageerror", (error) => problems.push(`page error: ${error.message}`));
  page.on("requestfailed", (request) => problems.push(`request failed: ${request.url()} ${request.failure()?.errorText}`));
  page.on("response", (response) => {
    if (response.status() >= 400) problems.push(`HTTP ${response.status()}: ${response.url()}`);
  });
  await page.goto(url, { waitUntil: "networkidle" });
  let text;
  if (click) {
    await page.click(click, { timeout: 10000 });
    text = await page.textContent(click);
    if (expected && !text.includes(expected)) problems.push(`after clicking ${click}: ${JSON.stringify(text)}, expected ${JSON.stringify(expected)}`);
  }
  const rendered = await page.evaluate(() => document.body.innerText.length);
  if (rendered === 0) problems.push("the page rendered no text");
  console.log(JSON.stringify({ ok: problems.length === 0, problems, text, rendered }));
} finally {
  await browser.close();
}
process.exit(problems.length === 0 ? 0 : 1);
