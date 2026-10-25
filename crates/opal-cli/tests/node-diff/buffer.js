// Buffer's byte-level edges, each printed so the two runtimes' answers can
// be compared line by line: encodings both ways, lenient base64 and hex,
// partial characters, fill, indexOf from every kind of offset, compare.
const show = (label, f) => {
  let out;
  try {
    out = f();
  } catch (e) {
    out = `${e.name} ${e.code} ${e.message}`;
  }
  console.log(label, out);
};
const encodings = ["utf8", "utf16le", "latin1", "ascii", "hex", "base64", "base64url"];
const samples = ["", "a", "hello", "héllo wörld", "\u{1F600}", "\ud800", "a\ud800b", "İÿĀ", "\0\x7f\x80\xff"];
for (const enc of encodings) {
  for (const s of samples) {
    show(`from ${enc} ${JSON.stringify(s)}`, () => Buffer.from(s, enc));
  }
}
const bytes = Buffer.from([0, 1, 0x7f, 0x80, 0xc3, 0xa9, 0xe2, 0x82, 0xac, 0xf0, 0x9f, 0x98, 0x80, 0xff, 0xfe, 0xc0, 0xaf, 0xed, 0xa0, 0x80]);
for (const enc of encodings) {
  show(`toString ${enc}`, () => JSON.stringify(bytes.toString(enc)));
  show(`toString ${enc} 3..9`, () => JSON.stringify(bytes.toString(enc, 3, 9)));
}
for (const s of ["YQ==", "YQ", "YQ=", "Y Q = =", "YQ==YQ==", "Y\nQ", "*Y*Q*", "-_-_", "+/+/", "YWJjİ", "Y", "=", "YQ=YQ", "aGVsbG8gd29ybGQ"]) {
  show(`base64 ${JSON.stringify(s)}`, () => Buffer.from(s, "base64"));
  show(`base64url ${JSON.stringify(s)}`, () => Buffer.from(s, "base64url"));
  show(`atob ${JSON.stringify(s)}`, () => JSON.stringify(atob(s)));
}
for (const s of ["", "0", "00", "0g", "abcdef", "ABCDEF", "abc", "zz00", "0İ", "12 34"]) {
  show(`hex ${JSON.stringify(s)}`, () => Buffer.from(s, "hex"));
}
show("btoa ok", () => btoa("hello\xff"));
show("btoa bad", () => btoa("Ā"));
show("byteLength", () => [Buffer.byteLength("héllo"), Buffer.byteLength("\u{1F600}"), Buffer.byteLength("aGk=", "base64"), Buffer.byteLength("abc", "hex")].join(","));
const small = Buffer.alloc(5);
show("write utf8 partial char", () => [small.write("ab\u{1F600}"), small].join(" "));
show("write ucs2 odd room", () => { const b = Buffer.alloc(5); return [b.write("abc", "utf16le"), b].join(" "); });
show("write hex invalid", () => { const b = Buffer.alloc(4); return [b.write("12zz34", "hex"), b].join(" "); });
show("write offset", () => { const b = Buffer.alloc(6, "."); return [b.write("xy", 2), b.toString()].join(" "); });
show("write offset length enc", () => { const b = Buffer.alloc(6, "."); return [b.write("abcdef", 1, 3, "latin1"), b.toString()].join(" "); });
show("write out of range", () => Buffer.alloc(2).write("a", 3));
show("fill string", () => Buffer.alloc(7).fill("abc"));
show("fill hex", () => Buffer.alloc(5).fill("0102", "hex"));
show("fill bad hex", () => Buffer.alloc(5).fill("zz", "hex"));
show("fill buffer", () => Buffer.alloc(5).fill(Buffer.from([1, 2])));
show("fill number", () => Buffer.alloc(3).fill(257));
show("fill range", () => Buffer.alloc(6).fill("x", 1, 4));
show("fill empty string", () => Buffer.alloc(3, 1).fill(""));
show("fill utf8 multibyte", () => Buffer.alloc(5).fill("é"));
const hay = Buffer.from("abcabcabc");
for (const [needle, offset] of [["abc", 0], ["abc", 1], ["abc", -3], ["abc", -100], ["abc", 100], ["", 0], ["", 100], ["", -2], ["c", 8], ["z", 0]]) {
  show(`indexOf ${needle} ${offset}`, () => hay.indexOf(needle, offset));
  show(`lastIndexOf ${needle} ${offset}`, () => hay.lastIndexOf(needle, offset));
  show(`includes ${needle} ${offset}`, () => hay.includes(needle, offset));
}
show("indexOf number", () => [hay.indexOf(99), hay.indexOf(99, 3), hay.lastIndexOf(97), hay.lastIndexOf(97, 5), hay.indexOf(355)].join(","));
show("indexOf buffer", () => [hay.indexOf(Buffer.from("ca")), hay.lastIndexOf(Buffer.from("ab"))].join(","));
const wide = Buffer.from("abcabc", "utf16le");
show("indexOf ucs2", () => [wide.indexOf("c", 0, "utf16le"), wide.lastIndexOf("a", undefined, "utf16le"), wide.indexOf("bc", 3, "ucs2")].join(","));
show("indexOf hex", () => hay.indexOf("6263", 0, "hex"));
show("compare", () => [Buffer.compare(Buffer.from("a"), Buffer.from("b")), Buffer.compare(Buffer.from("b"), Buffer.from("a")), Buffer.compare(Buffer.from("a"), Buffer.from("ab")), Buffer.from("abc").compare(Buffer.from("abc"))].join(","));
show("compare ranges", () => [Buffer.from("abcdef").compare(Buffer.from("xbcx"), 1, 3, 1, 3), Buffer.from("abc").compare(Buffer.from("abc"), 0, 2), Buffer.from("abc").compare(Buffer.from("abc"), 0, 3, 1)].join(","));
show("compare out of range", () => Buffer.from("abc").compare(Buffer.from("abc"), 0, 3, 5));
show("equals", () => [Buffer.from("x").equals(Buffer.from("x")), Buffer.from("x").equals(Buffer.from("y"))].join(","));
show("copy", () => { const t = Buffer.alloc(6, "."); const n = Buffer.from("abcdef").copy(t, 2, 1, 4); return `${n} ${t}`; });
show("copy overlapping", () => { const b = Buffer.from("abcdef"); b.copy(b, 2, 0, 4); return b.toString(); });
show("concat", () => Buffer.concat([Buffer.from("ab"), Buffer.from("cd")], 3).toString());
show("swap16", () => Buffer.from([1, 2, 3, 4]).swap16());
show("swap32", () => Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]).swap32());
show("swap64", () => Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]).swap64());
show("swap16 odd", () => Buffer.from([1, 2, 3]).swap16());
show("readers", () => { const b = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]); return [b.readUInt16LE(0), b.readInt32BE(2), b.readDoubleLE(0), b.readBigUInt64BE(0), b.readUIntBE(1, 3)].join(","); });
show("writers", () => { const b = Buffer.alloc(8); b.writeUInt32BE(0xdeadbeef, 0); b.writeInt16LE(-2, 4); b.writeUInt8(255, 6); return b; });
show("slice subarray", () => { const b = Buffer.from("abcdef"); const s = b.subarray(1, 3); s[0] = 0x5a; return `${b} ${s} ${b.slice(-2)}`; });
show("isUtf8", () => [require("buffer").isUtf8(Buffer.from("é")), require("buffer").isUtf8(Buffer.from([0xc3]))].join(","));
show("isAscii", () => [require("buffer").isAscii(Buffer.from("abc")), require("buffer").isAscii(Buffer.from("é"))].join(","));
show("toJSON", () => JSON.stringify(Buffer.from("hi")));
show("inspect", () => require("util").inspect(Buffer.from("hello world, this is a long buffer to see truncation past fifty bytes")));
show("alloc invalid", () => Buffer.alloc(-1));
show("from invalid", () => Buffer.from({}));
show("from arraybuffer", () => { const ab = new ArrayBuffer(4); new Uint8Array(ab)[1] = 7; return Buffer.from(ab, 1, 2); });
show("from array", () => Buffer.from([256, -1, 1.5, "7"]));
show("instanceof", () => [Buffer.from("x") instanceof Uint8Array, Buffer.isBuffer(Buffer.alloc(1)), Buffer.isBuffer(new Uint8Array(1)), Buffer.isEncoding("UTF-8"), Buffer.isEncoding("nope")].join(","));
show("equals pool", () => Buffer.from("a").buffer === Buffer.from("b").buffer);
show("constants", () => JSON.stringify(require("buffer").constants));
