// Checks that the web page's engine (web/engine-wasm.js) plays exactly like the CLI:
//  1. every fixture tests/defis/<cas>/ gives its expected.txt, through the CLI and through the
//     WebAssembly engine;
//  2. fuzz: random challenges, WebAssembly against `pantheon-battler api`, byte for byte.
// Usage: node scripts/verify-web.mjs [--n 2000] [--seed 1]
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const arg = (name, def) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? Number(process.argv[i + 1]) : def;
};
const N = arg("--n", 2000);
const SEED = arg("--seed", 1);
const exe = ["pantheon-battler.exe", "pantheon-battler"]
  .map((f) => join(root, "target", "release", f))
  .find(existsSync);
if (!exe) {
  console.error("erreur : CLI introuvable, lancez d'abord cargo build --release");
  process.exit(1);
}

// ---- The engine, loaded the way the page loads it.
const sandbox = { window: {} };
vm.runInNewContext(readFileSync(join(root, "web", "engine-wasm.js"), "utf8"), sandbox);
const bytes = Buffer.from(sandbox.window.PANTHEON_WASM, "base64");
const onDisk = readFileSync(join(root, "web", "engine.wasm"));
if (!bytes.equals(onDisk)) {
  console.error("erreur : web/engine-wasm.js ne correspond pas à web/engine.wasm (relancez embed-wasm.mjs)");
  process.exit(1);
}
let panic = null;
let memory;
const { instance } = await WebAssembly.instantiate(bytes, {
  env: {
    host_error(ptr, len) {
      panic = new TextDecoder().decode(new Uint8Array(memory.buffer, ptr, len));
    },
  },
});
const ex = instance.exports;
memory = ex.memory;
const enc = new TextEncoder();
const dec = new TextDecoder();
function callRaw(request) {
  const input = enc.encode(request);
  const p = ex.pb_alloc(input.length);
  new Uint8Array(memory.buffer, p, input.length).set(input);
  let r;
  try {
    r = ex.pb_call(p, input.length);
  } catch (e) {
    throw new Error(`le moteur WebAssembly a planté : ${panic || e.message}`);
  }
  ex.pb_free(p, input.length);
  const len = new DataView(memory.buffer).getUint32(r, true);
  const out = dec.decode(new Uint8Array(memory.buffer, r + 4, len));
  ex.pb_free(r, len + 4);
  return out;
}
const call = (req) => JSON.parse(callRaw(JSON.stringify(req)));

let failures = 0;
const fail = (msg) => {
  failures++;
  if (failures <= 10) console.error("ÉCHEC : " + msg);
};
const firstDiff = (x, y) => {
  let i = 0;
  while (i < x.length && x[i] === y[i]) i++;
  return `à l'octet ${i} : « ${JSON.stringify(x.slice(i, i + 60))} » contre « ${JSON.stringify(y.slice(i, i + 60))} »`;
};

const info = call({ op: "info" });
const cliHash = execFileSync(exe, ["api"], { input: '{"op":"info"}\n', encoding: "utf8" });
if (JSON.parse(cliHash).data_hash !== info.data_hash) fail(`données différentes : page ${info.data_hash}, CLI ${JSON.parse(cliHash).data_hash}`);
console.log(`moteur ${info.version} · données ${info.data_hash} · ${bytes.length} octets de WebAssembly`);

// ---- 1. Fixtures.
const defis = join(root, "tests", "defis");
const cases = existsSync(defis) ? readdirSync(defis).filter((d) => existsSync(join(defis, d, "expected.txt"))).sort() : [];
for (const c of cases) {
  const dir = join(defis, c);
  const read = (f) => readFileSync(join(dir, f), "utf8").replace(/\r\n/g, "\n");
  const dice = read("dice.txt").trim().split(/[\s,]+/).map(Number);
  const expected = read("expected.txt");
  const cli = execFileSync(exe, ["fight", "--a-file", join(dir, "a.txt"), "--b-file", join(dir, "b.txt"), "--dice", dice.join(","), "--forum"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
  });
  if (cli !== expected) fail(`${c} : la CLI ne donne plus expected.txt, ${firstDiff(cli, expected)}`);
  const page = call({ op: "fight", a: { text: read("a.txt") }, b: { text: read("b.txt") }, dice });
  if (!page.ok) fail(`${c} : ${page.errors.join(" ; ")}`);
  else if (page.forum + "\n" !== expected) fail(`${c} : la page diffère, ${firstDiff(page.forum + "\n", expected)}`);
}
console.log(`défis de référence : ${cases.length} vérifiés`);

// ---- 2. Fuzz: random challenges, WebAssembly against the native `api` command.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = rng(SEED);
const int = (lo, hi) => lo + Math.floor(rand() * (hi - lo + 1));
const pick = (xs) => xs[int(0, xs.length - 1)];
const names = ["Alice", "Bob", "Éloïse", "Zorg", "Ulrich", "Ma Ïa", ""];

function randomCompo() {
  const units = [];
  const n = rand() < 0.02 ? 5 : int(1, 4);
  for (let k = 0; k < n; k++) {
    const u = pick(info.roster);
    const star = u.star_allowed && rand() < 0.25;
    const p = star ? u.star_power : 1;
    const bump = () => (rand() < 0.5 ? 0 : int(0, 3));
    let [atk, hp, def] = [u.atk * p + bump(), u.hp * p + bump(), u.def * p + (rand() < 0.8 ? 0 : 1)];
    if (rand() < 0.01) hp = 0;
    const id = rand() < 0.15 ? u.name.normalize("NFD").replace(/[̀-ͯ]/g, "") : `${u.emoji} ${u.name}`;
    let line = `${k + 1}. ${star ? "⭐" : ""}${id} · ${atk}/${hp}/${def}`;
    if (u.offering) line += ` · Offrande ${int(0, u.offering)}/${u.offering}`;
    units.push(line);
  }
  const head = [];
  if (rand() < 0.95) head.push(`🏆 ${int(0, 8)}`);
  if (rand() < 0.95) head.push(`✨ ${int(1, 25)}`);
  if (rand() < 0.95) head.push(`série de ${rand() < 0.5 ? 0 : int(0, 8)} défaites`);
  const name = pick(names);
  const title = name ? `[b]⚔ Composition de ${name}[/b]` : "[b]⚔ Composition[/b]";
  return [[title, ...head].join(" · "), ...units, "{d100} {d100} {d100}"].join(rand() < 0.1 ? "\r\n" : "\n");
}

const requests = [];
for (let k = 0; k < N; k++) {
  const dice = Array.from({ length: 6 }, () => (rand() < 0.005 ? int(0, 101) : int(1, 100)));
  const req = { op: "fight", a: { text: randomCompo() }, b: { text: randomCompo() }, dice };
  if (rand() < 0.2) req.a.name = pick(names);
  requests.push(JSON.stringify(req));
}
// A few parse and format requests too.
for (let k = 0; k < 50; k++) requests.push(JSON.stringify({ op: "parse", text: randomCompo() }));
requests.push(JSON.stringify({ op: "info" }));
// Bazar shops: random levels, players, reserve and seed (sometimes invalid).
const unitIds = info.roster.map((u) => u.id);
for (let k = 0; k < 200; k++) {
  const levels = [1, 2, 3, 4].filter(() => rand() < 0.5);
  if (rand() < 0.05) levels.push(int(0, 6));
  const req = { op: "shop", levels, seed: int(0, 2 ** 31) * (rand() < 0.5 ? 1 : 4096) + int(0, 4095) };
  if (rand() < 0.5) req.players = rand() < 0.03 ? 0 : int(1, 30);
  if (rand() < 0.6) {
    req.stock = {};
    for (const id of unitIds) if (rand() < 0.7) req.stock[id] = rand() < 0.3 ? 0 : int(0, 12);
    if (rand() < 0.03) req.stock.licorne = 2;
  }
  if (rand() < 0.5) req.page = int(1, 300);
  requests.push(JSON.stringify(req));
}

const native = spawnSync(exe, ["api"], { input: requests.join("\n") + "\n", encoding: "utf8", maxBuffer: 1 << 30 });
if (native.status !== 0) {
  console.error("erreur : pantheon-battler api a échoué\n" + native.stderr);
  process.exit(1);
}
const answers = native.stdout.split("\n");
let played = 0;
let rejected = 0;
const outcomes = { A: 0, B: 0, nul: 0 };
requests.forEach((req, k) => {
  const page = callRaw(req);
  if (page !== answers[k]) fail(`requête ${k} : ${firstDiff(page, answers[k] || "")}\n  ${req}`);
  const a = JSON.parse(page);
  if (JSON.parse(req).op === "fight") {
    if (a.ok) {
      played++;
      outcomes[a.outcome]++;
    } else rejected++;
  }
});
console.log(`fuzz : ${requests.length} requêtes comparées (${played} combats joués : ${outcomes.A} A, ${outcomes.B} B, ${outcomes.nul} nuls ; ${rejected} refusés pour erreur)`);

if (failures) {
  console.error(`${failures} écart(s) entre la page et la CLI`);
  process.exit(1);
}
console.log("OK : la page et la CLI donnent exactement les mêmes résultats");
