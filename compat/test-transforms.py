#!/usr/bin/env python3
"""Projects whose transform opal doesn't do itself, run under Jest and under
opal test, and compared.

    python3 compat/test-transforms.py --opal PATH [PROJECT ...] [--keep]

opal transforms TypeScript, JSX and ES modules itself where that is what
babel-jest would do. Everything else goes through Jest's transformer
protocol, the project's own transformer called the way Jest calls it
(crates/opal-test/runtime/transformers.cjs): ts-jest, emitting only, with
its type-check done once for the run (runtime/checker.cjs); babel-jest
when the Babel config has more than opal does; and any other transformer.
The projects below exercise each of those:

  ts-jest            `preset: 'ts-jest'`: a type error in a test file, in a
                     module it imports, and in one a test requires and one
                     catches; a syntax error; a failing assertion under
                     erased type declarations (the source map); function
                     text as TypeScript prints it
  ts-jest-off        `diagnostics: false`: type errors don't fail
  ts-jest-warn       `diagnostics: {warnOnly: true}`: nor do these
  ts-jest-filters    `ignoreCodes` and `exclude`
  ts-jest-isolated   `isolatedModules` in tsconfig: syntax errors only
  ts-jest-decorators decorators with emitDecoratorMetadata, as Nest uses them
  ts-jest-globals    the options under globals['ts-jest'], another tsconfig
  babel-plugin       a Babel plugin of the project's own, jest.mock hoisting,
                     and a source-mapped failure through babel-jest
  babel-standard     preset-env and preset-react's automatic runtime, which
                     opal does itself
  babel-classic      preset-react's classic runtime: JSX without React in
                     scope fails
  babelrc-below      a .babelrc in a subfolder, which only that folder's
                     files get
  custom             a transformer module, a createTransformer factory with
                     options (and what TransformOptions holds), and one with
                     an inline source map
  transform-errors   a transformer that throws a SyntaxError, and one that
                     returns a string
  swc                @swc/jest, a transformer opal knows nothing about

Each project runs under Jest 30.5.2 and under opal test, both from cold
caches, and opal runs once more warm. What is compared, per test file:

  verdicts  pass, fail, skip or todo for every test, and the file's own
            pass or fail;
  message   the "Test suite failed to run" text of a file that failed to
            load, up to its stack, with the project's path replaced: a
            TSError, which has no stack, has to be ts-jest's to the byte;
  frames    for each failing test, the first stack frame in the project's
            own files, file:line:column, which is where source maps show.

opal's warm run has to give the same verdicts as its cold run. Then, where
EDITS says, files change, one step at a time, and opal's run after each
step, on its warm caches, has to give what Jest gives from cold: what the
type-check kept from the runs before must not outlive what it depended on.

  ts-jest-emit       is the one known difference: what transpiling file by
                     file, as ts-jest does under isolatedModules, can't see
                     of other files. KNOWN pins it, test by test, with why.

The pinned packages are in compat/test-transforms/ (`opal install` there).
A difference prints both sides and the run exits non-zero; --keep keeps the
projects.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEPS = HERE / "test-transforms" / "node_modules"
JEST = DEPS / "jest" / "bin" / "jest.js"

TSCONFIG = json.dumps({
    "compilerOptions": {
        "target": "es2020",
        "module": "commonjs",
        "strict": True,
        "esModuleInterop": True,
        "skipLibCheck": True,
    },
}, indent=2) + "\n"

SUM_TS = """export interface Pair {
  left: number;
  right: number;
}

export function sum({left, right}: Pair): number {
  return left + right;
}
"""

OK_TEST_TS = """import {expect, test} from '@jest/globals';
import {sum} from '../src/sum';

test('adds', () => {
  const total: number = sum({left: 1, right: 2});
  expect(total).toBe(3);
});

test('prints the function as TypeScript emits it', () => {
  expect(String(sum)).toBe('function sum({ left, right }) {\\n    return left + right;\\n}');
});
"""

MAPPED_TEST_TS = """import {expect, test} from '@jest/globals';

interface Shape {
  kind: string;
  sides: number;
}

type Named = {name: string};

test('fails on line 12', () => {
  const shape: Shape & Named = {kind: 'square', sides: 4, name: 'q'};
  expect(shape.sides).toBe(3);
});
"""

BAD_TEST_TS = """import {expect, test} from '@jest/globals';

const count: number = 'three';

test('never runs', () => {
  expect(count).toBeDefined();
});
"""

REPORTED_TEST_TS = """import {expect, test} from '@jest/globals';

const shape = {sides: 4};

test('never runs', () => {
  expect(shape.corners).toBeUndefined();
});
"""

SYNTAX_TEST_TS = """import {expect, test} from '@jest/globals';

const value = ;

test('never runs', () => {
  expect(value).toBeDefined();
});
"""

TS_PROJECTS = {
    "ts-jest": {
        "jest.config.js": "module.exports = {preset: 'ts-jest', testEnvironment: 'node'};\n",
        "tsconfig.json": TSCONFIG,
        "src/sum.ts": SUM_TS,
        "src/broken.ts": "export const value: string = 42;\n",
        "src/lazy-broken.ts": "export const value: string = 1;\n",
        "test/ok.test.ts": OK_TEST_TS,
        "test/mapped.test.ts": MAPPED_TEST_TS,
        "test/bad.test.ts": BAD_TEST_TS,
        "test/syntax.test.ts": SYNTAX_TEST_TS,
        "test/imports-bad.test.ts": """import {expect, test} from '@jest/globals';
import {value} from '../src/broken';

test('never runs', () => {
  expect(value).toBe(42);
});
""",
        "types/magic.d.ts": "declare const MAGIC: number;\n",
        "test/magic.test.ts": """import {expect, test} from '@jest/globals';

const read = (): number => (typeof MAGIC === 'undefined' ? 1 : MAGIC);

test('reads a global declaration', () => {
  expect(read()).toBe(1);
});
""",
        "test/lazy.test.ts": """import {expect, test} from '@jest/globals';

declare const require: (id: string) => {value: unknown};

test('requires a module with a type error', () => {
  expect(require('../src/lazy-broken').value).toBe(1);
});

test('catches the TSError', () => {
  expect(() => require('../src/lazy-broken')).toThrow('TS2322');
});

test('still runs', () => {
  expect(1).toBe(1);
});
""",
    },
    "ts-jest-off": {
        "jest.config.js": "module.exports = {transform: {'^.+\\\\.ts$': ['ts-jest', {diagnostics: false}]}};\n",
        "tsconfig.json": TSCONFIG,
        "src/sum.ts": SUM_TS,
        "test/ok.test.ts": OK_TEST_TS,
        "test/bad.test.ts": BAD_TEST_TS,
    },
    "ts-jest-warn": {
        "jest.config.js": "module.exports = {transform: {'^.+\\\\.ts$': ['ts-jest', {diagnostics: {warnOnly: true}}]}};\n",
        "tsconfig.json": TSCONFIG,
        "test/bad.test.ts": BAD_TEST_TS,
        "test/mapped.test.ts": MAPPED_TEST_TS,
    },
    "ts-jest-filters": {
        "jest.config.js": "module.exports = {transform: {'^.+\\\\.ts$': ['ts-jest', {diagnostics: {ignoreCodes: [2322], exclude: ['**/excluded/**']}}]}};\n",
        "tsconfig.json": TSCONFIG,
        "test/bad.test.ts": BAD_TEST_TS,
        "test/reported.test.ts": REPORTED_TEST_TS,
        "test/excluded/reported.test.ts": REPORTED_TEST_TS,
    },
    "ts-jest-isolated": {
        "jest.config.js": "module.exports = {preset: 'ts-jest'};\n",
        "tsconfig.json": TSCONFIG.replace('"strict": true', '"strict": true,\n    "isolatedModules": true'),
        "test/bad.test.ts": BAD_TEST_TS,
        "test/syntax.test.ts": SYNTAX_TEST_TS,
        "test/mapped.test.ts": MAPPED_TEST_TS,
    },
    # Nest's services, as its Jest scaffolds tested them: decorators with
    # the constructor's parameter types as metadata, a class from another
    # file and an interface.
    "ts-jest-decorators": {
        "jest.config.js": "module.exports = {preset: 'ts-jest', setupFiles: ['<rootDir>/reflect.js']};\n",
        "tsconfig.json": TSCONFIG.replace('"strict": true', '"strict": true,\n    "experimentalDecorators": true,\n    "emitDecoratorMetadata": true'),
        "reflect.js": """// reflect-metadata's two calls that TypeScript's __metadata and a test use.
const store = new WeakMap();
Reflect.metadata = (key, value) => target => {
  if (!store.has(target)) store.set(target, new Map());
  store.get(target).set(key, value);
};
Reflect.getMetadata = (key, target) => store.get(target)?.get(key);
""",
        "src/repository.ts": "export class Repository {\n  find(): string {\n    return 'found';\n  }\n}\n",
        "src/options.ts": "export interface Options {\n  verbose: boolean;\n}\n",
        "src/service.ts": """import {Repository} from './repository';
import {Options} from './options';

export function Injectable(): ClassDecorator {
  return () => {};
}

@Injectable()
export class Service {
  constructor(private readonly repository: Repository, readonly options: Options) {}

  run(): string {
    return this.repository.find();
  }
}
""",
        "test/service.test.ts": """import {expect, test} from '@jest/globals';
import {Repository} from '../src/repository';
import {Service} from '../src/service';

declare global {
  namespace Reflect {
    function getMetadata(key: string, target: object): unknown;
  }
}

test('the parameter types are metadata', () => {
  expect(Reflect.getMetadata('design:paramtypes', Service)).toEqual([Repository, Object]);
});

test('parameter properties are assigned', () => {
  expect(new Service(new Repository(), {verbose: true}).run()).toBe('found');
});
""",
    },
    # ts-jest's own emit is type-aware; opal's transpiles file by file, as
    # ts-jest does under isolatedModules. KNOWN pins what that changes.
    "ts-jest-emit": {
        "jest.config.js": "module.exports = {preset: 'ts-jest'};\n",
        "tsconfig.json": TSCONFIG,
        "src/types.ts": "export interface Shape {\n  sides: number;\n}\n",
        "src/index.ts": "export {Shape} from './types';\nexport const value = 1;\n",
        "src/levels.d.ts": "declare const enum Level {\n  Low = 1,\n  High = 2,\n}\n",
        "src/enum.ts": "export const enum Direction {\n  Up,\n  Down,\n}\n",
        "test/emit.test.ts": """import {expect, test} from '@jest/globals';
import * as lib from '../src/index';
import {Direction} from '../src/enum';

test('a type re-export is not an export', () => {
  expect(Object.keys(lib)).toEqual(['value']);
});

test('an ambient const enum is inlined', () => {
  expect(Level.High).toBe(2);
});

test('a const enum from another file has its values', () => {
  expect(Direction.Down).toBe(1);
});
""",
    },
    "ts-jest-globals": {
        "jest.config.js": "module.exports = {transform: {'^.+\\\\.ts$': 'ts-jest'}, globals: {'ts-jest': {tsconfig: 'tsconfig.test.json'}}};\n",
        "tsconfig.json": TSCONFIG,
        "tsconfig.test.json": TSCONFIG.replace('"strict": true', '"strict": false'),
        "test/loose.test.ts": """import {expect, test} from '@jest/globals';

function identity(value) {
  return value;
}

test('implicit any is fine without strict', () => {
  expect(identity(2)).toBe(2);
});
""",
    },
}

MAGIC_PLUGIN = """module.exports = ({types: t}) => ({
  visitor: {
    Identifier(path) {
      if (path.node.name === '__MAGIC__') path.replaceWith(t.numericLiteral(42));
    },
  },
});
"""

BABEL_PROJECTS = {
    "babel-plugin": {
        "babel.config.js": "module.exports = {presets: [['@babel/preset-env', {targets: {node: 'current'}}]], plugins: ['./magic-plugin']};\n",
        "magic-plugin.js": MAGIC_PLUGIN,
        "dep.js": "exports.value = 'real';\n",
        "magic.test.js": """import dep from './dep';

jest.mock('./dep', () => ({__esModule: true, default: {value: 'mocked'}}));

test('the plugin ran', () => {
  expect(__MAGIC__).toBe(42);
});

test('jest.mock is hoisted above the import', () => {
  expect(dep.value).toBe('mocked');
});

test('fails on line 16', () => {
  expect(__MAGIC__).toBe(41);
});
""",
    },
    "babel-standard": {
        "babel.config.js": "module.exports = {presets: [['@babel/preset-env', {targets: {node: 'current'}}], ['@babel/preset-react', {runtime: 'automatic'}]]};\n",
        "jsx.test.jsx": """test('renders without React in scope', () => {
  const element = <div className="x" />;
  expect(element.props.className).toBe('x');
});

test('fails on line 7', () => {
  expect(<span />).toBe(null);
});
""",
    },
    "babel-classic": {
        "babel.config.js": "module.exports = {presets: ['@babel/preset-react']};\n",
        "without-react.test.jsx": """test('needs React in scope', () => {
  expect((<div />).type).toBe('div');
});
""",
        "with-react.test.jsx": """const React = require('react');

test('has React in scope', () => {
  expect((<div />).type).toBe('div');
});
""",
    },
    "babelrc-below": {
        "babel.config.js": "module.exports = {presets: [['@babel/preset-env', {targets: {node: 'current'}}]]};\n",
        "magic-plugin.js": MAGIC_PLUGIN,
        "special/.babelrc": json.dumps({"plugins": ["../magic-plugin"]}) + "\n",
        "special/magic.test.js": """test('the .babelrc here applies', () => {
  expect(__MAGIC__).toBe(42);
});
""",
        "plain.test.js": """test('and not here', () => {
  expect(typeof __MAGIC__).toBe('undefined');
});
""",
    },
}

OTHER_PROJECTS = {
    "custom": {
        "jest.config.js": """module.exports = {
  moduleFileExtensions: ['js', 'json', 'txt', 'data', 'mapped'],
  transform: {
    '\\\\.txt$': '<rootDir>/upper-transformer.js',
    '\\\\.data$': ['<rootDir>/factory-transformer.js', {suffix: '!'}],
    '\\\\.mapped$': '<rootDir>/mapped-transformer.js',
    '\\\\.[jt]sx?$': 'babel-jest',
  },
};
""",
        "upper-transformer.js": """const crypto = require('crypto');

module.exports = {
  getCacheKey(source, filename) {
    return crypto.createHash('sha1').update(source).update(filename).digest('hex');
  },
  process(source) {
    return {code: `module.exports = ${JSON.stringify(source.trim().toUpperCase())};`};
  },
};
""",
        "factory-transformer.js": """const path = require('path');

module.exports = {
  createTransformer(options) {
    return {
      process(source, filename, transformOptions) {
        const seen = {
          text: source.trim() + options.suffix,
          file: path.basename(filename),
          rootDir: path.basename(transformOptions.config.rootDir),
          configString: typeof transformOptions.configString,
          transformerConfig: transformOptions.transformerConfig,
          instrument: transformOptions.instrument,
          supportsStaticESM: transformOptions.supportsStaticESM,
          supportsDynamicImport: transformOptions.supportsDynamicImport,
          cacheFS: transformOptions.cacheFS instanceof Map,
        };
        return {code: `module.exports = ${JSON.stringify(seen)};`};
      },
    };
  },
};
""",
        # Puts three lines of its own first, and says so in an inline source
        # map: each line of the file maps to itself, three lines up.
        "mapped-transformer.js": """const VLQ = ['A', 'C'];

module.exports = {
  process(source, filename) {
    const lines = source.split('\\n');
    const mappings = ';;;' + lines.map((_, i) => (i === 0 ? 'AAAA' : 'AA' + VLQ[1] + 'A')).join(';');
    const map = {version: 3, sources: [filename], names: [], mappings};
    const header = '// one\\n// two\\n// three\\n';
    const comment = '//# sourceMappingURL=data:application/json;charset=utf-8;base64,' + Buffer.from(JSON.stringify(map)).toString('base64');
    return {code: header + source + '\\n' + comment};
  },
};
""",
        "hello.txt": "hello world\n",
        "thing.data": "thing\n",
        "failing.mapped": """test('fails on line 1', () => { expect(1).toBe(2); });
""",
        "text.test.js": """test('a module transformer', () => {
  expect(require('./hello.txt')).toBe('HELLO WORLD');
});

test('a factory, its options and what Jest passes it', () => {
  expect(require('./thing.data')).toEqual({
    text: 'thing!',
    file: 'thing.data',
    rootDir: 'custom',
    configString: 'string',
    transformerConfig: {suffix: '!'},
    instrument: false,
    supportsStaticESM: false,
    supportsDynamicImport: false,
    cacheFS: true,
  });
});
""",
        "mapped.test.js": "require('./failing.mapped');\n",
    },
    "transform-errors": {
        "jest.config.js": """module.exports = {
  transform: {
    '\\\\.broken\\\\.js$': '<rootDir>/throwing-transformer.js',
    '\\\\.bad\\\\.js$': '<rootDir>/string-transformer.js',
    '\\\\.js$': 'babel-jest',
  },
};
""",
        "throwing-transformer.js": """module.exports = {
  process() {
    throw new SyntaxError('Unexpected token (1:5)');
  },
};
""",
        "string-transformer.js": """module.exports = {
  process(source) {
    return source;
  },
};
""",
        "thing.broken.js": "module.exports = 1;\n",
        "thing.bad.js": "module.exports = 1;\n",
        "broken.test.js": "require('./thing.broken.js');\ntest('never runs', () => {});\n",
        "bad.test.js": "require('./thing.bad.js');\ntest('never runs', () => {});\n",
    },
    "swc": {
        "jest.config.js": "module.exports = {transform: {'^.+\\\\.(t|j)sx?$': '@swc/jest'}};\n",
        "dep.ts": "export const value: string = 'real';\n",
        "swc.test.ts": """import {value} from './dep';

jest.mock('./dep', () => ({value: 'mocked'}));

interface Unused {
  field: number;
}

test('jest.mock is hoisted', () => {
  expect(value).toBe('mocked');
});

test('fails on line 14', () => {
  expect(value).toBe('real');
});
""",
    },
}

PROJECTS = {**TS_PROJECTS, **BABEL_PROJECTS, **OTHER_PROJECTS}

# Edits made after opal's warm run, for a third run on its warm caches,
# compared with Jest's from cold: a module whose return type changes, which
# is a type error only in the test that imports it, and a global declaration
# that changes type under a test that doesn't import it. Jest from cold is what CI sees; a warm Jest wouldn't check either test
# again, as ts-jest only checks the files it transforms.
EDITS = {
    "ts-jest": [
        {"src/sum.ts": SUM_TS.replace("): number {\n  return left + right;", "): string {\n  return String(left + right);")},
        {"types/magic.d.ts": "declare const MAGIC: string;\n"},
    ],
}

# Where opal differs from Jest on purpose: per project and file, the tests
# whose verdict differs and what opal's is. A pin has to hold exactly: a
# difference that grows, shrinks or goes away fails the run, as an edit to
# the pin is a decision, not a retry.
KNOWN = {
    "ts-jest-emit": {
        "test/emit.test.ts": {
            "tests": {
                "a type re-export is not an export": "fail",
                "an ambient const enum is inlined": "fail",
            },
            "why": "transpileModule can't tell a type from a value in another file: it keeps "
                   "`export {Shape} from` as an export, and leaves an ambient const enum a "
                   "reference to a global that doesn't exist (SEMANTICS.md, Transforms)",
        },
    },
}

STATUS = {"passed": "pass", "failed": "fail", "pending": "skip", "skipped": "skip", "todo": "todo", "disabled": "skip"}


def lay_out(root, files):
    shutil.rmtree(root, ignore_errors=True)
    for rel, text in files.items():
        target = root / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    (root / "package.json").write_text(json.dumps({"name": root.name, "private": True}) + "\n")
    (root / "node_modules").symlink_to(DEPS)


# Where Jest's message ends and its stack starts: a code frame, or the first
# frame. opal prints the frames but no code frame (COMPATIBILITY.md), so the
# message is compared without either.
STACK_START = re.compile(r"\n\s*(?:>?\s*\d+ \||at \S)")


def message_of(text, root):
    text = text.replace(str(root), "<root>")
    match = STACK_START.search(text)
    return (text[: match.start()] if match else text).rstrip()


def frame(message, root):
    """The first stack frame in the project's own files: file:line:column."""
    for match in re.finditer(r"(?:\(|at )(/[^\s()]+):(\d+):(\d+)\)?", message):
        path = Path(match.group(1))
        try:
            rel = path.relative_to(root)
        except ValueError:
            continue
        if "node_modules" in rel.parts:
            continue
        return f"{rel.as_posix()}:{match.group(2)}:{match.group(3)}"
    return None


def read_report(report, root):
    """Per test file: the file's verdict, each test's, the message of a file
    that failed to load, and each failing test's first frame."""
    results = {}
    for entry in json.loads(report.read_text())["testResults"]:
        tests, frames = {}, {}
        for test in entry["assertionResults"]:
            title = " > ".join([*test["ancestorTitles"], test["title"]])
            tests[title] = STATUS.get(test["status"], test["status"])
            if test.get("failureMessages"):
                frames[title] = frame("\n".join(test["failureMessages"]), root)
        message = entry.get("message", "") if not entry["assertionResults"] else ""
        results[Path(entry["name"]).relative_to(root).as_posix()] = {
            "file": "fail" if entry["status"] == "failed" else "pass",
            "tests": tests,
            "message": message_of(message, root),
            "frames": frames,
        }
    return results


def run(cmd, root, env, log):
    with open(log, "w") as out:
        subprocess.run(cmd, cwd=root, env=env, stdout=out, stderr=subprocess.STDOUT, timeout=600)


def pinned(reference, pin):
    """The reference's results with a pin's verdicts in place of its own:
    what opal has to give."""
    expected = json.loads(json.dumps(reference))
    expected["tests"].update(pin["tests"])
    if any(verdict == "fail" for verdict in expected["tests"].values()):
        expected["file"] = "fail"
    for title in pin["tests"]:
        expected["frames"].pop(title, None)
    return expected


def compare(name, reference, observed, labels, pins=None):
    """Every difference between two runs' results, as text. A pinned file's
    frames for its pinned tests aren't compared: opal fails those where Jest
    doesn't."""
    problems = []
    for file in sorted(set(reference) | set(observed)):
        ours, theirs = observed.get(file), reference.get(file)
        if ours is None or theirs is None:
            problems.append(f"{name}/{file}: only {labels[0] if ours is None else labels[1]} ran it")
            continue
        pin = (pins or {}).get(file)
        if pin:
            theirs = pinned(theirs, pin)
            ours = {**ours, "frames": {k: v for k, v in ours["frames"].items() if k not in pin["tests"]}}
        for key in ("file", "tests", "message", "frames"):
            if ours[key] != theirs[key]:
                what = f"{labels[0]}, as pinned" if pin else labels[0]
                problems.append(f"{name}/{file} {key}:\n  {what}: {theirs[key]!r}\n  {labels[1]}: {ours[key]!r}")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--opal", required=True, type=Path)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("projects", nargs="*")
    args = parser.parse_args()
    args.opal = args.opal.resolve()
    if not JEST.exists():
        sys.exit("needs `opal install` in compat/test-transforms")
    names = args.projects or list(PROJECTS)
    scratch = Path(tempfile.mkdtemp(prefix="opal-test-transforms-"))
    env = {k: v for k, v in os.environ.items() if k not in ("FORCE_COLOR", "NO_COLOR", "TS_JEST_LOG")}
    problems = []
    try:
        for name in names:
            root = scratch / name
            lay_out(root, PROJECTS[name])
            jest_report, opal_report, warm_report = root / ".jest.json", root / ".opal.json", root / ".opal-warm.json"
            run(["node", str(JEST), "--ci", "--json", f"--outputFile={jest_report}",
                 f"--cacheDirectory={scratch / '.jest-cache' / name}"], root, env, root / ".jest.log")
            opal = [str(args.opal), "test", "--ci", "--json", f"--cache-dir={scratch / '.opal-cache' / name}"]
            run([*opal, f"--outputFile={opal_report}"], root, env, root / ".opal.log")
            run([*opal, f"--outputFile={warm_report}"], root, env, root / ".opal-warm.log")
            if not jest_report.exists() or not opal_report.exists() or not warm_report.exists():
                problems.append(f"{name}: a run wrote no report (see {root})")
                print(f"{name}: no report", flush=True)
                continue
            reference = read_report(jest_report, root)
            found = compare(name, reference, read_report(opal_report, root), ("Jest", "opal"), KNOWN.get(name))
            found += compare(name, read_report(opal_report, root), read_report(warm_report, root), ("opal", "opal, warm"))
            before = reference
            for step, edits in enumerate(EDITS.get(name, []), 1):
                for rel, text in edits.items():
                    (root / rel).write_text(text)
                edited_jest, edited_opal = root / f".jest-edit{step}.json", root / f".opal-edit{step}.json"
                run(["node", str(JEST), "--ci", "--json", f"--outputFile={edited_jest}",
                     f"--cacheDirectory={scratch / '.jest-cache' / f'{name}-edit{step}'}"], root, env, root / f".jest-edit{step}.log")
                run([*opal, f"--outputFile={edited_opal}"], root, env, root / f".opal-edit{step}.log")
                if not edited_jest.exists() or not edited_opal.exists():
                    found.append(f"{name}: a run after edit {step} wrote no report (see {root})")
                    break
                edited = read_report(edited_jest, root)
                if edited == before:
                    found.append(f"{name}: edit {step} changed nothing under Jest, so it tests nothing")
                found += compare(name, edited, read_report(edited_opal, root), (f"Jest after edit {step}", f"opal after edit {step}"))
                before = edited
            verdicts = sum(len(r["tests"]) + 1 for r in reference.values())
            jest = [v for r in reference.values() for v in r["tests"].values()]
            failed_to_run = sum(1 for r in reference.values() if not r["tests"] and r["file"] == "fail")
            verdict = "differs as pinned" if name in KNOWN else "agrees"
            print(f"{name}: {verdict if not found else f'{len(found)} differences'} ({len(reference)} files, {verdicts} verdicts; "
                  f"under Jest {jest.count('pass')} tests pass, {jest.count('fail')} fail, {failed_to_run} files fail to run)", flush=True)
            problems += found
    finally:
        if args.keep:
            print(f"kept {scratch}")
        else:
            shutil.rmtree(scratch, ignore_errors=True)
    for problem in problems:
        print(f"\n{problem}")
    sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
