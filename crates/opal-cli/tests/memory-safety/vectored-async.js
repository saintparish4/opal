// A readv on the thread pool (over 1 MiB, so it leaves the loop) while
// JavaScript transfers its buffer away and fills the heap with buffers of the
// same size. The request holds the buffer's backing store, so the kernel
// writes into memory that is still the request's, and none of the new
// buffers is touched. Node 24.19.0 fails this with EFAULT: its request
// memory was freed under it.
const fs = require("fs");
const path = require("path");

const SIZE = 4 << 20;
const file = path.join(process.cwd(), "vectored-async.data");
fs.writeFileSync(file, Buffer.alloc(SIZE, 0x61));
const fd = fs.openSync(file, "r");

const victim = new Uint8Array(new ArrayBuffer(SIZE));
const churn = [];
fs.readv(fd, [victim], 0, (error, n) => {
  const intact = churn.every((buffer) => buffer.every((byte) => byte === 0x63));
  console.log(error ? error.code : "ok", n, intact);
  fs.closeSync(fd);
  fs.unlinkSync(file);
});
victim.buffer.transfer(16);
for (let i = 0; i < 16; i++) churn.push(new Uint8Array(SIZE).fill(0x63));
