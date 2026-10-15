const express = require("express");

const app = express();
const port = Number(process.env.PORT) || 3000;

app.get("/health", (req, res) => {
  res.status(200).json({ status: "ok" });
});

app.listen(port, () => console.log(`listening on ${port}`));
