// Checks a bundle's source map with tools that are not opal's: every
// identifier in the bundle is found with TypeScript's scanner and looked up
// in the map with @jridgewell/trace-mapping.
//
//   node sourcemap-check.mjs <bundle.mjs>
//
// The rules, per identifier:
//
// - One that carries the bundler's prefix is the bundler's own and is not
//   checked. Neither is any identifier on a line with no mapping at all:
//   that is the runtime the bundle carries, which has no original.
// - One with a mapping of its own (a segment starting exactly at it) must
//   point into the right source at a real position. For JavaScript, which
//   the bundler copies, the original text there must be the same
//   identifier, or the original name the map records when the bundle
//   renamed it. For TypeScript and JSX, which a compiler rewrote, the
//   compiler may map code it generated to the construct it came from, so
//   the position must be real but need not hold the same token.
// - One with no mapping of its own, on a mapped line, fails, unless no
//   original contains it (a compiler introduced it) or it is one of the
//   property names the bundler's own templates write.
//
// Prints a summary, and exits 1 with the first failures.

import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { TraceMap, eachMapping } from "@jridgewell/trace-mapping";

const require = createRequire(import.meta.url);
const ts = require("typescript");

// Property names in emit.rs's templates: `.then` after the entry's
// evaluation promise, `.href` after `new URL`.
const TEMPLATE_NAMES = new Set(["then", "href"]);

const bundlePath = process.argv[2];
const code = readFileSync(bundlePath, "utf8");
const map = new TraceMap(JSON.parse(readFileSync(bundlePath + ".map", "utf8")));
const prefix = /const (\w+)Object = Object/.exec(code)?.[1];
if (!prefix) {
  console.error("no bundler prefix found");
  process.exit(1);
}

const sources = new Map();
map.sources.forEach((source, index) => {
  if (source != null && map.sourcesContent?.[index] != null) sources.set(source, map.sourcesContent[index]);
});
const lineStarts = new Map();
for (const [source, text] of sources) {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") starts.push(i + 1);
  lineStarts.set(source, starts);
}
const contains = (text, name) => new RegExp(`(^|[^\\w$])${name.replace(/\$/g, "\\$")}($|[^\\w$])`).test(text);

// Exact segments: "line:column" of the generated position. And for each
// generated line, the source its segments come from, which is the module
// that line was printed from.
const segments = new Map();
const lineSource = new Map();
eachMapping(map, (mapping) => {
  if (mapping.source == null) return;
  segments.set(`${mapping.generatedLine}:${mapping.generatedColumn}`, mapping);
  if (!lineSource.has(mapping.generatedLine)) lineSource.set(mapping.generatedLine, mapping.source);
});

const bundleLineStarts = [0];
for (let i = 0; i < code.length; i++) if (code[i] === "\n") bundleLineStarts.push(i + 1);
const position = (offset) => {
  let low = 0;
  let high = bundleLineStarts.length - 1;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (bundleLineStarts[mid] <= offset) low = mid;
    else high = mid - 1;
  }
  return { line: low + 1, column: offset - bundleLineStarts[low] };
};

// After these tokens a slash divides; anywhere else it starts a regular
// expression.
const endsExpression = new Set([
  ts.SyntaxKind.Identifier,
  ts.SyntaxKind.NumericLiteral,
  ts.SyntaxKind.BigIntLiteral,
  ts.SyntaxKind.StringLiteral,
  ts.SyntaxKind.RegularExpressionLiteral,
  ts.SyntaxKind.NoSubstitutionTemplateLiteral,
  ts.SyntaxKind.TemplateTail,
  ts.SyntaxKind.CloseParenToken,
  ts.SyntaxKind.CloseBracketToken,
  ts.SyntaxKind.CloseBraceToken,
  ts.SyntaxKind.ThisKeyword,
  ts.SyntaxKind.SuperKeyword,
  ts.SyntaxKind.TrueKeyword,
  ts.SyntaxKind.FalseKeyword,
  ts.SyntaxKind.NullKeyword,
  ts.SyntaxKind.PlusPlusToken,
  ts.SyntaxKind.MinusMinusToken,
]);
const compiled = (source) => /\.(ts|tsx|mts|cts|jsx)$/.test(source);

const scanner = ts.createScanner(ts.ScriptTarget.ESNext, false, ts.LanguageVariant.Standard, code);
const counts = { exact: 0, compiled: 0, synthesized: 0, bundler: 0, runtime: 0 };
const failures = [];
let previous = ts.SyntaxKind.Unknown;
let beforePrevious = ts.SyntaxKind.Unknown;
let depth = 0;
const templates = [];
for (
  let kind = scanner.scan();
  kind !== ts.SyntaxKind.EndOfFileToken;
  beforePrevious = previous, previous = kind, kind = scanner.scan()
) {
  if ((kind === ts.SyntaxKind.SlashToken || kind === ts.SyntaxKind.SlashEqualsToken) && !endsExpression.has(previous)) {
    kind = scanner.reScanSlashToken();
  }
  if (kind === ts.SyntaxKind.OpenBraceToken) depth++;
  if (kind === ts.SyntaxKind.CloseBraceToken) {
    if (templates.length > 0 && templates[templates.length - 1] === depth) {
      kind = scanner.reScanTemplateToken(false);
      if (kind === ts.SyntaxKind.TemplateTail) templates.pop();
      continue;
    }
    depth--;
  }
  if (kind === ts.SyntaxKind.TemplateHead) {
    templates.push(depth);
    continue;
  }
  if (kind !== ts.SyntaxKind.Identifier) continue;
  const text = scanner.getTokenValue();
  // `meta` in `import.meta` and `target` in `new.target` are part of a meta
  // property, which prints and maps as one token.
  if (
    previous === ts.SyntaxKind.DotToken &&
    (beforePrevious === ts.SyntaxKind.ImportKeyword || beforePrevious === ts.SyntaxKind.NewKeyword)
  ) {
    continue;
  }
  const at = position(scanner.getTokenStart());
  if (text.startsWith(prefix)) {
    counts.bundler++;
    continue;
  }
  const module = lineSource.get(at.line);
  if (module == null) {
    counts.runtime++;
    continue;
  }
  const segment = segments.get(`${at.line}:${at.column}`);
  if (!segment) {
    // A compiler writes names that have no token of their own (a JSX
    // property key, an enum's own name in its lowering); for a module the
    // bundler copied, only its templates do.
    if (TEMPLATE_NAMES.has(text) || compiled(module) || !contains(sources.get(module) ?? "", text)) {
      counts.synthesized++;
      continue;
    }
    failures.push(`${at.line}:${at.column} ${text}: no mapping of its own`);
    continue;
  }
  const source = sources.get(segment.source);
  const starts = lineStarts.get(segment.source);
  if (source == null || segment.originalLine > starts.length) {
    failures.push(`${at.line}:${at.column} ${text}: maps to ${segment.source}:${segment.originalLine}, which is not in the source`);
    continue;
  }
  const offset = starts[segment.originalLine - 1] + segment.originalColumn;
  const lineEnd = segment.originalLine < starts.length ? starts[segment.originalLine] : source.length + 1;
  if (offset >= lineEnd) {
    failures.push(`${at.line}:${at.column} ${text}: maps past the end of ${segment.source}:${segment.originalLine}`);
    continue;
  }
  const expected = segment.name ?? text;
  if (source.slice(offset, offset + expected.length) === expected) {
    counts.exact++;
  } else if (compiled(segment.source)) {
    counts.compiled++;
  } else if (!contains(source, text) && segment.name == null) {
    counts.synthesized++;
  } else {
    failures.push(
      `${at.line}:${at.column} ${text}: maps to ${segment.source}:${segment.originalLine}:${segment.originalColumn}, which reads ${JSON.stringify(source.slice(offset, offset + 24))}`,
    );
  }
}
console.log(
  `sourcemap: ${counts.exact} identifiers round-trip exactly, ${counts.compiled} map into compiled code, ${counts.synthesized} introduced by a compiler or template, ${counts.bundler + counts.runtime} the bundler's own, ${failures.length} failed`,
);
if (failures.length > 0) {
  for (const failure of failures.slice(0, 20)) console.log("  " + failure);
  process.exit(1);
}
