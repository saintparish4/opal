// readvSync and writevSync with an array whose second element is a getter
// that frees the first element's memory, after lib/fs.js has validated the
// array (its first read) and while the binding reads it (its second).
//
// Until 0.4.3 the binding turned each element into a raw pointer as it went,
// so the getter freed memory already in an iovec and readv wrote 64 KiB into
// it: a heap use-after-free under AddressSanitizer. Node 24.19.0 does the
// same (it returns 65536 here). Opal reads every element first and then holds
// each one's backing store, so the freed buffer is detached, empty, and
// skipped, and only the 8 live bytes are read or written.
const fs = require("fs");
const path = require("path");

const SIZE = 1 << 16;
const file = path.join(process.cwd(), "vectored-sync.data");
fs.writeFileSync(file, Buffer.alloc(SIZE, 0x61));

function attack(op) {
  const victim = new Uint8Array(new ArrayBuffer(SIZE));
  victim.fill(0x62);
  const buffers = [victim, null];
  let calls = 0;
  Object.defineProperty(buffers, 1, {
    enumerable: true,
    get() {
      calls += 1;
      // Transferring to another length frees the old memory.
      if (calls === 2) victim.buffer.transfer(16);
      return new Uint8Array(8);
    },
  });
  const fd = fs.openSync(file, op === "read" ? "r" : "r+");
  const n = op === "read" ? fs.readvSync(fd, buffers, 0) : fs.writevSync(fd, buffers, 0);
  fs.closeSync(fd);
  console.log(op, n, victim.byteLength, calls);
}

attack("read");
attack("write");
fs.unlinkSync(file);
