// A CommonJS entry that throws at the top level: the process sees an
// uncaught exception, not a rejection.
process.on("exit", (code) => console.log("exit", code));
Promise.resolve().then(() => console.log("microtask before the throw was handled"));
console.log("about to throw");
throw new RangeError("from the entry");
