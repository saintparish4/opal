#!/usr/bin/env bash
# Copies the files listed in FILES out of the pinned Node release's own
# library, unmodified. They are read from the node binary itself
# (process.binding('natives'), the sources it was built from), so what is
# vendored is exactly what that release runs.
#
#   crates/opal-runtime/js/node/update.sh
#
# The release is the one compare-runtimes.py and compat/ pin: a ratio to
# "node" and a conformance claim both mean that one node.
set -euo pipefail
cd "$(dirname "$0")"
PINNED=v24.19.0
actual="$(node --version)"
if [ "$actual" != "$PINNED" ]; then
  echo "update.sh: node is $actual, the pinned release is $PINNED" >&2
  exit 1
fi
node --no-deprecation - <<'JS'
const fs = require("fs");
const path = require("path");
const natives = process.binding("natives");
const files = fs.readFileSync("FILES", "utf8").split("\n").map((l) => l.replace(/#.*/, "").trim()).filter(Boolean);
for (const id of files) {
  if (!(id in natives)) throw new Error(`${id} is not a module of this node`);
  fs.mkdirSync(path.dirname(`${id}.js`), { recursive: true });
  fs.writeFileSync(`${id}.js`, natives[id]);
}
console.log(`vendored ${files.length} files from node ${process.version}`);
JS
