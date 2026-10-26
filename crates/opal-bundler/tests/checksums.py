#!/usr/bin/env python3
"""Bundle every fixture and print the SHA-256 of each bundle and map.

SEMANTICS.md rule 16 says a bundle is a function of the graph, the
options, and the opal version. CI runs this on Linux and macOS, from a
temporary directory whose path differs between them, publishes both
outputs as artifacts, and fails if they differ.

    python3 crates/opal-bundler/tests/checksums.py --opal target/debug/opal > checksums.txt
"""

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

FIXTURES = Path(__file__).resolve().parent / "fixtures"
ENTRIES = ["main.mjs", "main.js", "main.cjs", "main.ts", "main.mts", "main.cts", "main.tsx", "main.jsx"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--opal", required=True)
    args = parser.parse_args()
    opal = str(Path(args.opal).resolve())
    lines = []
    with tempfile.TemporaryDirectory(prefix="opal-checksums-") as work:
        for fixture in sorted(FIXTURES.iterdir()):
            if not fixture.is_dir() or (fixture / "hand-bundle.mjs").exists():
                continue
            config = json.loads((fixture / "fixture.json").read_text()) if (fixture / "fixture.json").exists() else {}
            if "refused" in json.dumps(config.get("expect", "")):
                continue
            program = Path(work) / fixture.name
            shutil.copytree(fixture, program, symlinks=True)
            entry = config.get("entry") or next(name for name in ENTRIES if (program / name).exists())
            command = [opal, "build", str(program / entry), "--outdir", str(program / "dist"), "--cache-dir", str(Path(work) / "cache")]
            command += ["--target", config.get("target", "node")]
            for external in config.get("external", []):
                command += ["--external", external]
            for path, value in config.get("define", {}).items():
                command += ["--define", f"{path}={value}"]
            built = subprocess.run(command, capture_output=True, text=True)
            if built.returncode != 0:
                print(f"{fixture.name}: build failed\n{built.stderr}", file=sys.stderr)
                return 1
            for output in sorted((program / "dist").iterdir()):
                digest = hashlib.sha256(output.read_bytes()).hexdigest()
                lines.append(f"{digest}  {fixture.name}/{output.name}")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
