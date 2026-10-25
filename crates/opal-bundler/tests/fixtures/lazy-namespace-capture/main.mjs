// Found by the generated-graph fuzzer (seed 1002770): a lazy module that
// re-exports another module's `b` and declares its own `b`. Its namespace
// must read the re-exported one.
import "./shared.mjs";
import("./lazy.mjs").then((ns) => console.log(Object.keys(ns), ns.b));
