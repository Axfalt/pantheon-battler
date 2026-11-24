// Embeds web/engine.wasm in web/engine-wasm.js (base64), so that the page works from file://
// and on any static host, without fetch() or a WebAssembly MIME type.
// Usage: PANTHEON_GIT=<commit> node scripts/embed-wasm.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const wasm = readFileSync(join(root, "web", "engine.wasm"));
const git = (process.env.PANTHEON_GIT || "inconnu").replace(/[^\p{L}\w.+-]/gu, "");
const out =
  "// Généré par scripts/embed-wasm.mjs à partir de web/engine.wasm : ne pas modifier à la main.\n" +
  `window.PANTHEON_GIT = ${JSON.stringify(git)};\n` +
  `window.PANTHEON_WASM = "${wasm.toString("base64")}";\n`;
writeFileSync(join(root, "web", "engine-wasm.js"), out);
console.log(`web/engine-wasm.js : ${out.length} octets (moteur ${wasm.length} octets, commit ${git})`);
