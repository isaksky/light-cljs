// A line-oriented nREPL client for tests/dev_nrepl.rs.
//
//   node nrepl_client.mjs HOST PORT
//
// Each line of stdin is a JSON object, sent as a bencoded dictionary:
// strings as byte strings (UTF-8), integers as integers, arrays as lists and
// objects as dictionaries. Each message the server sends is printed as one
// line of JSON on stdout, byte strings as UTF-8 text. The client exits when
// the server closes the connection; the end of stdin closes it.

import net from "node:net";
import readline from "node:readline";

function encode(value) {
  if (typeof value === "string") {
    const bytes = Buffer.from(value, "utf8");
    return Buffer.concat([Buffer.from(`${bytes.length}:`), bytes]);
  }
  if (typeof value === "number") {
    if (!Number.isInteger(value)) throw new Error(`not an integer: ${value}`);
    return Buffer.from(`i${value}e`);
  }
  if (Array.isArray(value)) {
    return Buffer.concat([Buffer.from("l"), ...value.map(encode), Buffer.from("e")]);
  }
  if (value && typeof value === "object") {
    const keys = Object.keys(value).sort();
    const parts = keys.flatMap((key) => [encode(key), encode(value[key])]);
    return Buffer.concat([Buffer.from("d"), ...parts, Buffer.from("e")]);
  }
  throw new Error(`cannot bencode ${JSON.stringify(value)}`);
}

// The value at `at` of `buffer` and where it ends, or null when the buffer
// ends first.
function decode(buffer, at) {
  if (at >= buffer.length) return null;
  const first = buffer[at];
  if (first === 0x69 /* i */) {
    const end = buffer.indexOf(0x65 /* e */, at);
    if (end < 0) return null;
    return { value: Number(buffer.toString("latin1", at + 1, end)), at: end + 1 };
  }
  if (first >= 0x30 && first <= 0x39) {
    const colon = buffer.indexOf(0x3a /* : */, at);
    if (colon < 0) return null;
    const length = Number(buffer.toString("latin1", at, colon));
    if (colon + 1 + length > buffer.length) return null;
    return {
      value: buffer.toString("utf8", colon + 1, colon + 1 + length),
      at: colon + 1 + length,
    };
  }
  if (first === 0x6c /* l */ || first === 0x64 /* d */) {
    const items = [];
    let next = at + 1;
    for (;;) {
      if (next >= buffer.length) return null;
      if (buffer[next] === 0x65) break;
      const item = decode(buffer, next);
      if (!item) return null;
      items.push(item.value);
      next = item.at;
    }
    if (first === 0x6c) return { value: items, at: next + 1 };
    const dict = {};
    for (let index = 0; index + 1 < items.length; index += 2) {
      dict[items[index]] = items[index + 1];
    }
    return { value: dict, at: next + 1 };
  }
  throw new Error(`not bencode at byte ${at}: ${buffer[at]}`);
}

const [host, port] = process.argv.slice(2);
const socket = net.connect({ host, port: Number(port) });
let pending = Buffer.alloc(0);
socket.on("data", (chunk) => {
  pending = Buffer.concat([pending, chunk]);
  for (;;) {
    const message = decode(pending, 0);
    if (!message) break;
    pending = pending.subarray(message.at);
    process.stdout.write(`${JSON.stringify(message.value)}\n`);
  }
});
socket.on("close", () => process.exit(0));
socket.on("error", (error) => {
  process.stderr.write(`nrepl_client: ${error.message}\n`);
  process.exit(1);
});
socket.on("connect", () => {
  const lines = readline.createInterface({ input: process.stdin });
  lines.on("line", (line) => {
    if (line.trim()) socket.write(encode(JSON.parse(line)));
  });
  lines.on("close", () => socket.end());
});
