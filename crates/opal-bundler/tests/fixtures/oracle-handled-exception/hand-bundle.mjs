process.on("uncaughtException", (error) => {
  console.error("handled:", error.message);
  process.exitCode = 4;
});
setTimeout(() => console.error("still running"), 0);
throw new Error("boom");
