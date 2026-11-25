// Local test server: the site (web/) and the Bazar API (/api/…) on one port, with the store in
// memory. Nothing leaves this computer.
//
//   node server/dev.mjs [--port 8787] [--data bazar-dev.json] [--key dev] [--fresh] [--mh-appkey KEY]
//
// The data is kept in bazar-dev.json (gitignored) between two runs; --fresh starts empty.
// MyHordes login: without --mh-appkey, a fake MyHordes where the key "test-Alice" is the player
// Alice; with it, the real MyHordes API.
// Players: http://localhost:8787/ (Bazar tab) · GM tool: http://localhost:8787/gm_tool/
// (Partage tab: server http://localhost:8787/api, key "dev").

import { createServer } from "node:http";
import { readFile, stat, rm } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { createHandler } from "./handler.mjs";
import { memoryStore } from "./store-memory.mjs";
import { fakeMyHordes, myHordesVerifier } from "./myhordes.mjs";

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const port = Number(opt("port", 8787));
const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const dataFile = resolve(root, opt("data", "bazar-dev.json"));
if (args.includes("--fresh")) await rm(dataFile, { force: true });
const gmKey = opt("key", "dev");
const web = resolve(fileURLToPath(new URL("../web", import.meta.url)));

const mhAppKey = opt("mh-appkey", "");
const handle = createHandler(memoryStore({ file: dataFile }), { gmKey, myHordes: mhAppKey ? myHordesVerifier({ appKey: mhAppKey }) : fakeMyHordes() });
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".wasm": "application/wasm",
  ".ico": "image/x-icon",
};

async function serveFile(res, urlPath) {
  let p = normalize(join(web, decodeURIComponent(urlPath)));
  if (p !== web && !p.startsWith(web + sep)) return send(res, 403, "text/plain", "Interdit");
  try {
    if ((await stat(p)).isDirectory()) p = join(p, "index.html");
    const body = await readFile(p);
    send(res, 200, types[extname(p)] || "application/octet-stream", body);
  } catch (e) {
    send(res, 404, "text/plain; charset=utf-8", "Introuvable");
  }
}

function send(res, status, type, body) {
  res.writeHead(status, { "content-type": type, "cache-control": "no-store" });
  res.end(body);
}

createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  if (url.pathname.startsWith("/api/") || url.pathname === "/api") {
    // Any page may call the local API (another local preview, 127.0.0.1, a file opened from disk).
    res.setHeader("access-control-allow-origin", req.headers.origin || "*");
    res.setHeader("access-control-allow-headers", "authorization, content-type");
    res.setHeader("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS");
    res.setHeader("access-control-allow-private-network", "true");
    res.setHeader("vary", "origin");
    if (req.method === "OPTIONS") {
      res.writeHead(204);
      return res.end();
    }
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const r = await handle({
      method: req.method,
      path: url.pathname.slice(4) || "/",
      query: Object.fromEntries(url.searchParams),
      headers: req.headers,
      body: Buffer.concat(chunks).toString("utf8"),
    });
    return send(res, r.status, "application/json; charset=utf-8", JSON.stringify(r.body));
  }
  if (url.pathname === "/gm_tool") {
    res.writeHead(301, { location: "/gm_tool/" });
    return res.end();
  }
  serveFile(res, url.pathname);
}).listen(port, () => {
  console.log(`Pantheon Battler en local : http://localhost:${port}/  (outil MJ : http://localhost:${port}/gm_tool/)`);
  console.log(mhAppKey ? "Connexion MyHordes : la vraie API MyHordes" : "Connexion MyHordes simulée : la clé « test-Alice » est la joueuse Alice");
  console.log(`API du Bazar : http://localhost:${port}/api · clé MJ « ${gmKey} »` + ` · données gardées dans ${dataFile} (--fresh pour repartir de zéro)`);
});
