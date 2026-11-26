// Pantheon Battler: the Bazar server. Players confirm their Bazar posts here, so that a unit
// bought by one is gone for the others; the GM tools publish the current shop and the common
// reserve, and import the players' posts into their journal.
//
// Everything is an event with a server sequence number (`seq`): a confirmed post, or the
// cancellation of one. The store keeps the events, the current shop, who bought each slot of
// it, and the copies available in the common reserve (the GM's reserve plus every event the GM
// tool hasn't imported yet).
//
// Players have an account holding their god (favors, level, field, reserve) and their posts.
// An account is reached with a session token the browser keeps (sent as `Authorization: Bearer
// s.…`). It is made either from the player's MyHordes key (optional: the name is then proven by
// MyHordes) or, without a key, at the first confirmed post (the name is then the one typed, kept
// for that account only). A code (another session token) opens the account on another device.
// Every write of an account carries a version number: two devices can't overwrite each other.
// The account's posts (with their text, the account's history) are a separate item, written only
// by a confirmation or a cancellation, so that saving the god stays a small write. Everything
// expires `ttlDays` after its last write.
//
// The GMs' journal is kept here too: every GM action (a new game, a shop drawn, a correction, an
// undo, a player's post imported) is an event the GM tools append, numbered in arrival order,
// each event id once (a resend is skipped). The GM tools replay it to get the reserve.
//
// Routes (JSON, French error messages in `error`):
//   GET    /bazar[?since=N]        public: shop, bought slots, available copies, last events
//   GET    /events?after=N         public: events after seq N (the GM tool imports them)
//   POST   /accounts               a new account without MyHordes { name, state } → token
//   POST   /login/myhordes         { key, state } → token (links the current account if any)
//   GET    /me                     the account: name, god, version, last posts
//   PUT    /me                     { version, state?, name? }
//   POST   /sessions               a code for another device
//   DELETE /sessions               log this device out
//   DELETE /sessions/others        log every other device out (a code given away)
//   POST   /posts                  the player confirms a post
//   POST   /posts/<seq>/cancel     its player or a GM cancels it
//   PUT    /shop, PUT /stock       GM: the current shop, the common reserve
//   GET    /gm                     GM: key check, what the server already has
//   GET    /gm/journal?after=N     GM: the journal's events numbered after N
//   POST   /gm/journal             GM: { events: [...] } appended (ids already there skipped)

import { createHash, randomBytes } from "node:crypto";

export class Conflict extends Error {
  constructor(kind, info = {}) {
    super(kind);
    this.kind = kind; // "seq" | "slot" | "stock" | "shop" | "cancelled" | "account" | "name" | "mh"
    this.info = info;
  }
}

const MAX_BODY = 32 * 1024;
const MAX_TEXT = 6000;
const GM_EVENT = 16 * 1024; // one GM journal event, as JSON
const GM_BATCH = 25; // GM journal events per request
const UNIT_RE = /^[a-z0-9_]{1,40}$/;
const RETRIES = 8;
const FEED = 20; // events a page gets when it first loads
const POSTS = 30; // posts kept in an account's history
const DAY = 86400 * 1000;

const clean = (s, max) =>
  String(s ?? "")
    .replace(/[\u0000-\u001f\u007f[\]{}]/g, "")
    .trim()
    .slice(0, max);
export const fold = (s) => clean(s, 40).toLocaleLowerCase("fr");
const isInt = (n, min, max) => Number.isInteger(n) && n >= min && n <= max;
const hash = (token) => createHash("sha256").update(token).digest("hex");
const newToken = () => "s." + randomBytes(24).toString("base64url");

/// What an event does to the copies available: unit id → change.
export function delta(e) {
  const d = {};
  const add = (id, n) => (d[id] = (d[id] || 0) + n);
  if (e.type === "post") {
    if (e.buy) add(e.buy.unit, -1);
    for (const id of e.returned || []) add(id, 1);
  } else if (e.type === "cancel") {
    if (e.buy) add(e.buy.unit, 1);
    for (const id of e.returned || []) add(id, -1);
  }
  for (const k of Object.keys(d)) if (d[k] === 0) delete d[k];
  return d;
}

class HttpError extends Error {
  constructor(status, message, extra = {}) {
    super(message);
    this.status = status;
    this.extra = extra;
  }
}

/// A god as the engine reads it (bazar_post::PlayerState), checked and trimmed.
function cleanState(s, name) {
  if (!s || typeof s !== "object" || Array.isArray(s)) throw new HttpError(400, "État du dieu illisible.");
  const num = (v, min, max, what) => {
    v = v ?? min;
    if (!isInt(v, min, max)) throw new HttpError(400, `État du dieu illisible (${what}).`);
    return v;
  };
  const entry = (e) => {
    if (!e || typeof e !== "object" || typeof e.id !== "string" || !UNIT_RE.test(e.id)) throw new HttpError(400, "État du dieu illisible (unité).");
    return { id: e.id, star: !!e.star, atk: num(e.atk, 0, 9999, "⚔"), hp: num(e.hp, 0, 9999, "❤"), def: num(e.def, 0, 9999, "🛡"), deaths: num(e.deaths, 0, 9999, "morts") };
  };
  const list = (l) => {
    if (l == null) return [];
    if (!Array.isArray(l) || l.length > 12) throw new HttpError(400, "État du dieu illisible (terrain ou réserve).");
    return l.map(entry);
  };
  return {
    name,
    favors: num(s.favors, 0, 1000000, "faveurs"),
    level: num(s.level, 1, 10, "niveau"),
    invested: num(s.invested, 0, 1000000, "investi"),
    board: list(s.board),
    reserve: list(s.reserve),
    // The arena numbers, for the challenge composition (Compte tab); absent = not given.
    ...Object.fromEntries(
      [["vp", 0, "🏆"], ["essence", 0, "✨"], ["streak", 0, "défaites"], ["wins", 0, "victoires"]]
        .filter(([k]) => s[k] != null)
        .map(([k, min, what]) => [k, num(s[k], min, 9999, what)]),
    ),
  };
}

/// `store`: see store-memory.mjs. `gmKey`: the secret the GM tools send in Authorization.
/// `myHordes`: a verifier (myhordes.mjs), or null when the server has no MyHordes app key.
export function createHandler(store, { gmKey, now = () => Date.now(), myHordes = null, ttlDays = 60 } = {}) {
  const bearer = (headers) => {
    const h = headers.authorization || headers.Authorization || "";
    return h.startsWith("Bearer ") ? h.slice(7) : "";
  };
  const isGm = (headers) => !!gmKey && bearer(headers) === gmKey;
  const needGm = (headers) => {
    if (!isGm(headers)) throw new HttpError(401, "Clé MJ absente ou fausse.");
  };

  // ------------------------------------------------------------------ Accounts
  async function who(headers) {
    const token = bearer(headers);
    if (!/^s\.[\w-]{20,80}$/.test(token)) return null;
    const session = await store.session(hash(token));
    const account = session && (await store.account(session.account));
    // "Log the other devices out" raises the account's epoch: older sessions stop working.
    if (!account || (session.epoch || 0) !== (account.epoch || 0)) return null;
    return { account, session: hash(token) };
  }
  async function needPlayer(headers) {
    const w = await who(headers);
    if (!w) throw new HttpError(401, "Connexion perdue ou expirée : reconnectez-vous (clé MyHordes, ou code d'un autre appareil).", { session: false });
    return w;
  }
  // What a page gets of an account; `posts` only when they were read (or just written).
  const view = (a, posts) => ({
    account: { id: a.id, name: a.name, verified: !!a.mh, created: a.created, updated: a.updated, expires: a.updated + ttlDays * DAY },
    state: a.state,
    version: a.version,
    ...(posts ? { posts } : {}),
  });
  // What saving an account also writes: its name (kept for it), the session in use, its
  // MyHordes link (`links`), its posts when they change. Each write renews their 60 days. A plain
  // save of the god renews the links once a day only (`due`), to keep it a single small write.
  const writes = (a, { expectVersion, session = null, claim = true, links = true, takeover = false, release = null, posts = undefined }) => ({
    item: a,
    expectVersion,
    claim: claim && links ? { name: fold(a.name), verified: !!a.mh, takeover } : null,
    release,
    session: links ? session : null,
    mh: links && a.mh ? a.mh.id : null,
    posts,
  });
  const due = (a) => now() - (a.refreshed || 0) > DAY;
  const nameTaken = (name) =>
    new HttpError(409, `Le nom « ${name} » est déjà pris par un autre joueur. Si c'est vous : connectez-vous avec votre clé MyHordes, ou avec le code d'un autre appareil.`);

  async function createAccount(body) {
    const name = clean(body.name, 40);
    if (!name) throw new HttpError(400, "Indiquez votre nom sur le forum.");
    const owner = await store.nameOwner(fold(name));
    if (owner) throw nameTaken(name);
    const t = now();
    const a = { id: "l-" + randomBytes(9).toString("base64url"), name, mh: null, state: cleanState(body.state || {}, name), version: 1, epoch: 0, created: t, updated: t, refreshed: t };
    const token = newToken();
    try {
      await store.saveAccount(writes(a, { expectVersion: null, session: hash(token), posts: [] }));
    } catch (e) {
      if (e instanceof Conflict && e.kind === "name") throw nameTaken(name);
      throw e;
    }
    return { ok: true, token, ...view(a, []) };
  }

  async function loginMyHordes(body, headers) {
    if (!myHordes) throw new HttpError(501, "La connexion MyHordes n'est pas encore ouverte sur ce Bazar : continuez sans clé.");
    const key = String(body.key ?? "").trim();
    if (key.length < 4 || key.length > 200) throw new HttpError(400, "Clé MyHordes manquante.");
    const mh = await myHordes(key);
    if (mh.error) throw new HttpError(mh.status || 401, mh.error);
    const name = clean(mh.name, 40);
    const current = await who(headers);
    const token = newToken();
    for (let attempt = 0; attempt < 3; attempt++) {
      const t = now();
      const linkedId = await store.mhAccount(mh.id);
      const linked = linkedId && (await store.account(linkedId));
      let base;
      let expectVersion;
      if (linked) {
        // This MyHordes player already has a god here: open it.
        base = linked;
        expectVersion = linked.version;
      } else if (current && !current.account.mh) {
        // The god of this browser becomes this MyHordes player's.
        base = current.account;
        expectVersion = base.version;
      } else {
        base = { id: "mh-" + mh.id, mh: null, state: cleanState(body.state || {}, name), version: 0, epoch: 0, created: t };
        expectVersion = null;
      }
      const a = { ...base, name, mh: { id: mh.id, name }, state: { ...base.state, name }, version: base.version + 1, updated: t, refreshed: t };
      const old = base.name && fold(base.name) !== fold(name) ? fold(base.name) : null;
      const release = old && (await store.nameOwner(old))?.account === a.id ? old : null;
      try {
        // A MyHordes name is proven: it takes the name from a god made without a key.
        await store.saveAccount(writes(a, { expectVersion, session: hash(token), takeover: true, release, posts: expectVersion == null ? [] : undefined }));
        return { ok: true, token, ...view(a, expectVersion == null ? [] : await store.posts(a.id)) };
      } catch (e) {
        if (!(e instanceof Conflict)) throw e;
        if (e.kind === "name") throw new HttpError(409, `Le nom « ${name} » appartient déjà à un autre compte MyHordes.`);
        // account or mh: someone saved meanwhile, again.
      }
    }
    throw new HttpError(503, "Le Bazar est très occupé : réessayez dans un instant.");
  }

  async function putMe(body, headers) {
    const w = await needPlayer(headers);
    const a = w.account;
    if (body.version !== a.version) throw stale();
    let name = a.name;
    let release = null;
    if (body.name !== undefined && clean(body.name, 40) !== a.name) {
      if (a.mh) throw new HttpError(400, "Le nom d'un compte MyHordes est celui de MyHordes.");
      name = clean(body.name, 40);
      if (!name) throw new HttpError(400, "Indiquez votre nom sur le forum.");
      const owner = await store.nameOwner(fold(name));
      if (owner && owner.account !== a.id) throw nameTaken(name);
      if (fold(name) !== fold(a.name) && (await store.nameOwner(fold(a.name)))?.account === a.id) release = fold(a.name);
    }
    const state = body.state !== undefined ? cleanState(body.state, name) : { ...a.state, name };
    const links = !!release || name !== a.name || due(a);
    const next = { ...a, name, state, version: a.version + 1, updated: now(), ...(links ? { refreshed: now() } : {}) };
    try {
      await store.saveAccount(writes(next, { expectVersion: a.version, session: w.session, release, links }));
    } catch (e) {
      if (!(e instanceof Conflict)) throw e;
      if (e.kind === "name") throw nameTaken(name);
      if (e.kind === "account") throw stale();
      throw e;
    }
    return { ok: true, ...view(next) };
  }
  const stale = () =>
    new HttpError(409, "Votre dieu a changé ailleurs (autre appareil, ou message annulé par un MJ) : il vient d'être rechargé, vérifiez-le.", { stale: true });

  async function newSession(headers) {
    const w = await needPlayer(headers);
    const token = newToken();
    await store.putSession(hash(token), w.account.id, w.account.epoch || 0);
    return { ok: true, token };
  }

  // Every other device (and every code given) stops working; this one goes on.
  async function dropOthers(headers) {
    const w = await needPlayer(headers);
    const a = w.account;
    const next = { ...a, epoch: (a.epoch || 0) + 1, version: a.version + 1, updated: now(), refreshed: now() };
    try {
      await store.saveAccount(writes(next, { expectVersion: a.version, session: w.session }));
    } catch (e) {
      if (e instanceof Conflict && e.kind === "account") throw stale();
      throw e;
    }
    return { ok: true, ...view(next) };
  }

  // ------------------------------------------------------------------ The Bazar
  // A page polls this every few seconds: with `since` (the last event it has), only the newer
  // events are read, so a poll with nothing new costs a few small reads (the table's capacity is
  // the free tier's). Without it (first load, or too far behind), the last FEED events.
  async function bazar(query) {
    const snap = await store.snapshot();
    const slots = snap.shop ? await store.slots(snap.shop.id) : [];
    const since = query.since === undefined || query.since === "" ? null : Number(query.since);
    const fresh = since !== null && isInt(since, 0, 1e12) && since <= snap.seq && snap.seq - since <= FEED;
    const events = !fresh ? await store.events({ last: FEED }) : since === snap.seq ? [] : await store.events({ after: since, limit: FEED });
    return { seq: snap.seq, shop: snap.shop, avail: snap.avail, slots, events, since: fresh ? since : null, myhordes: !!myHordes, at: now() };
  }

  // A post: { version, shop, buy?: {level, slot, unit}, returned: [ids], invest, fee, price,
  // sales, after, forum }. The god before it is the account's.
  async function confirm(body, headers) {
    const w = await needPlayer(headers);
    const returned = Array.isArray(body.returned) ? body.returned : [];
    if (returned.length > 24 || !returned.every((id) => typeof id === "string" && UNIT_RE.test(id)))
      throw new HttpError(400, "Liste des unités rendues illisible.");
    const forum = typeof body.forum === "string" ? body.forum.slice(0, MAX_TEXT) : "";
    if (!forum) throw new HttpError(400, "Le texte du message manque.");
    const nums = {};
    for (const k of ["invest", "fee", "price", "sales"]) {
      const v = body[k] ?? 0;
      if (!isInt(v, 0, 100000)) throw new HttpError(400, `Nombre invalide : ${k}.`);
      nums[k] = v;
    }
    for (let attempt = 0; attempt < RETRIES; attempt++) {
      const a = attempt ? await store.account(w.account.id) : w.account;
      if (!a || body.version !== a.version) throw stale();
      const after = cleanState(body.after, a.name);
      const snap = await store.snapshot();
      let buy = null;
      if (body.buy) {
        const { level, slot, unit } = body.buy;
        if (!snap.shop) throw new HttpError(409, "Aucune boutique n'est ouverte pour l'instant.");
        if (body.shop !== snap.shop.id)
          throw new HttpError(409, `La boutique a changé (page ${snap.shop.page}) : rechargez-la et refaites votre choix.`);
        const shop = snap.shop.shops.find((s) => s.level === level);
        if (!shop || !isInt(slot, 0, 99) || shop.units[slot] !== unit)
          throw new HttpError(409, "Cette unité n'est pas à cet emplacement de la boutique : rechargez-la.");
        buy = { level, slot, unit, page: snap.shop.page };
      }
      if (snap.avail) {
        for (const id of [...returned, ...(buy ? [buy.unit] : [])])
          if (!(id in snap.avail)) throw new HttpError(400, `Unité inconnue de la réserve : ${id}.`);
      }
      const t = now();
      const event = {
        seq: snap.seq + 1,
        type: "post",
        at: t,
        player: a.name,
        account: a.id,
        verified: !!a.mh,
        shop: buy ? snap.shop.id : (snap.shop && snap.shop.id) || null,
        page: snap.shop ? snap.shop.page : null,
        buy,
        returned,
        ...nums,
        before: a.state,
        after,
        version: a.version + 1, // the account's version right after this post
        forum,
      };
      const posts = [{ seq: event.seq, at: t, page: event.page, forum, cancelled: null }, ...(await store.posts(a.id))].slice(0, POSTS);
      const next = { ...a, state: after, version: a.version + 1, updated: t, refreshed: t };
      try {
        await store.commit({
          expectSeq: snap.seq,
          event,
          slot: buy ? { shop: snap.shop.id, level: buy.level, slot: buy.slot, player: a.name, seq: event.seq } : null,
          shopId: buy ? snap.shop.id : null,
          delta: snap.avail ? delta(event) : null,
          account: writes(next, { expectVersion: a.version, session: w.session, posts }),
        });
        return { ok: true, event, ...view(next, posts) };
      } catch (e) {
        if (!(e instanceof Conflict)) throw e;
        if (e.kind === "seq") continue;
        if (e.kind === "account") throw stale();
        if (e.kind === "name")
          throw new HttpError(409, `Le nom « ${a.name} » appartient maintenant à un compte vérifié par MyHordes : changez de nom, ou connectez-vous avec votre clé MyHordes.`);
        if (e.kind === "slot") throw new HttpError(409, `Trop tard : ${e.info.player || "un autre joueur"} l'a déjà achetée.`);
        if (e.kind === "stock") throw new HttpError(409, "Trop tard : la réserve commune n'a plus d'exemplaire de cette unité.");
        if (e.kind === "shop") throw new HttpError(409, "La boutique vient de changer : rechargez-la et refaites votre choix.");
        throw e;
      }
    }
    throw new HttpError(503, "Le Bazar est très occupé : réessayez dans un instant.");
  }

  // Cancelling gives the unit back to the shop and the sold units back to the player. The
  // player's god goes back to what it was before the post if it hasn't changed since.
  async function cancel(seq, body, headers) {
    const gm = isGm(headers);
    const w = gm ? null : await needPlayer(headers);
    for (let attempt = 0; attempt < RETRIES; attempt++) {
      const snap = await store.snapshot();
      const target = await store.event(seq);
      if (!target || target.type !== "post") throw new HttpError(404, `Message n°${seq} introuvable.`);
      if (target.cancelled) throw new HttpError(409, `Le message n°${seq} est déjà annulé.`);
      if (!gm && target.account !== w.account.id) throw new HttpError(403, `Seul ${target.player} peut annuler ce message.`);
      const by = gm ? clean(body.by, 40) || "MJ" : w.account.name;
      const t = now();
      const event = { seq: snap.seq + 1, type: "cancel", at: t, player: target.player, account: target.account || null, by, gm, target: seq, buy: target.buy, returned: target.returned };
      const owner = target.account ? await store.account(target.account) : null;
      let next = null;
      let posts = null;
      let restored = false;
      if (owner) {
        restored = owner.version === target.version && !!target.before;
        next = { ...owner, state: restored ? { ...target.before, name: owner.name } : owner.state, version: owner.version + 1, updated: t };
        posts = (await store.posts(owner.id)).map((p) => (p.seq === seq ? { ...p, cancelled: { seq: event.seq, at: t, by, gm }, restored } : p));
      }
      try {
        await store.commit({
          expectSeq: snap.seq,
          event,
          freeSlot: target.buy ? { shop: target.shop, level: target.buy.level, slot: target.buy.slot, seq } : null,
          cancels: seq,
          delta: snap.avail ? delta(event) : null,
          account: next ? writes(next, { expectVersion: owner.version, session: w ? w.session : null, claim: false, posts }) : null,
        });
        return { ok: true, event, restored, ...(w && next ? view(next, posts) : {}) };
      } catch (e) {
        if (!(e instanceof Conflict)) throw e;
        if (e.kind === "seq" || e.kind === "account") continue;
        if (e.kind === "cancelled") throw new HttpError(409, `Le message n°${seq} est déjà annulé.`);
        if (e.kind === "stock") throw new HttpError(409, "Impossible d'annuler : une unité vendue dans ce message a déjà été rachetée.");
        throw e;
      }
    }
    throw new HttpError(503, "Le Bazar est très occupé : réessayez dans un instant.");
  }

  async function putShop(body) {
    const { id, page, shops } = body;
    if (typeof id !== "string" || !/^[\w.-]{1,80}$/.test(id)) throw new HttpError(400, "Identifiant de boutique invalide.");
    if (!isInt(page, 0, 1000000)) throw new HttpError(400, "Numéro de page invalide.");
    if (!Array.isArray(shops) || shops.length > 10) throw new HttpError(400, "Boutiques illisibles.");
    const list = shops.map((s) => {
      if (!isInt(s.level, 1, 10) || !Array.isArray(s.units) || s.units.length > 20 || !s.units.every((u) => typeof u === "string" && UNIT_RE.test(u)))
        throw new HttpError(400, "Boutiques illisibles.");
      return { level: s.level, units: s.units };
    });
    const shop = { id, page, shops: list, at: isInt(body.at, 0, 1e15) ? body.at : now(), by: String(body.by || "MJ").slice(0, 40) };
    await store.putShop(shop);
    return { ok: true, shop };
  }

  // The GM's reserve includes every event up to `through`; the later ones are added on top.
  async function putStock(body) {
    const counts = body.counts;
    if (!counts || typeof counts !== "object" || Array.isArray(counts)) throw new HttpError(400, "Réserve illisible.");
    const entries = Object.entries(counts);
    if (entries.length > 200 || !entries.every(([id, n]) => UNIT_RE.test(id) && isInt(n, 0, 999))) throw new HttpError(400, "Réserve illisible.");
    if (!isInt(body.through, 0, 1e12)) throw new HttpError(400, "Numéro d'événement invalide (through).");
    for (let attempt = 0; attempt < RETRIES; attempt++) {
      const snap = await store.snapshot();
      const through = Math.min(body.through, snap.seq);
      const avail = { ...counts };
      for (const e of await store.events({ after: through })) {
        if (e.seq > snap.seq) break;
        for (const [id, n] of Object.entries(delta(e))) if (id in avail) avail[id] = Math.max(0, avail[id] + n);
      }
      try {
        await store.putStock({ expectSeq: snap.seq, stock: { counts, through, at: now() }, avail });
        return { ok: true, avail, seq: snap.seq };
      } catch (e) {
        if (e instanceof Conflict && e.kind === "seq") continue;
        throw e;
      }
    }
    throw new HttpError(503, "Le Bazar est très occupé : réessayez dans un instant.");
  }

  // The GMs' journal: the events are the GM tool's own (the server only checks their size and id).
  async function gmAppend(body) {
    const events = body.events;
    if (!Array.isArray(events) || events.length > GM_BATCH) throw new HttpError(400, `Journal illisible (au plus ${GM_BATCH} événements par envoi).`);
    const ids = new Set();
    for (const e of events) {
      if (!e || typeof e !== "object" || Array.isArray(e) || typeof e.id !== "string" || !/^[\w.-]{1,80}$/.test(e.id))
        throw new HttpError(400, "Événement du journal illisible (id).");
      if (JSON.stringify(e).length > GM_EVENT) throw new HttpError(413, "Événement du journal trop gros.");
      if (ids.has(e.id)) throw new HttpError(400, "Événement en double dans l'envoi.");
      ids.add(e.id);
    }
    for (let attempt = 0; attempt < RETRIES; attempt++) {
      try {
        return { ok: true, ...(await store.gmAppend(events)) };
      } catch (e) {
        if (e instanceof Conflict && e.kind === "seq") continue;
        throw e;
      }
    }
    throw new HttpError(503, "Le journal est très occupé : réessayez dans un instant.");
  }

  /// One request: { method, path, query, headers, body (string) } → { status, body (object) }.
  return async function handle(req) {
    const headers = req.headers || {};
    try {
      const path = (req.path || "/").replace(/\/+$/, "") || "/";
      let body = {};
      if (req.method === "POST" || req.method === "PUT") {
        const raw = req.body || "";
        if (raw.length > MAX_BODY) throw new HttpError(413, "Requête trop grosse.");
        try {
          body = raw ? JSON.parse(raw) : {};
        } catch (e) {
          throw new HttpError(400, "Requête illisible (JSON).");
        }
        if (!body || typeof body !== "object") throw new HttpError(400, "Requête illisible.");
      }
      const ok = async (p) => ({ status: 200, body: await p });
      const route = `${req.method} ${path}`;
      let m;
      if (route === "GET /bazar") return await ok(bazar(req.query || {}));
      if (route === "GET /events") {
        const after = Number((req.query || {}).after || 0);
        if (!isInt(after, 0, 1e12)) throw new HttpError(400, "Paramètre after invalide.");
        const events = await store.events({ after, limit: 500 });
        return await ok({ events, more: events.length === 500 });
      }
      if (route === "POST /accounts") return await ok(createAccount(body));
      if (route === "POST /login/myhordes") return await ok(loginMyHordes(body, headers));
      if (route === "GET /me") {
        const w = await needPlayer(headers);
        return await ok({ ok: true, ...view(w.account, await store.posts(w.account.id)) });
      }
      if (route === "PUT /me") return await ok(putMe(body, headers));
      if (route === "POST /sessions") return await ok(newSession(headers));
      if (route === "DELETE /sessions/others") return await ok(dropOthers(headers));
      if (route === "DELETE /sessions") {
        const token = bearer(headers);
        if (token.startsWith("s.")) await store.dropSession(hash(token));
        return await ok({ ok: true });
      }
      if (route === "POST /posts") return await ok(confirm(body, headers));
      if (req.method === "POST" && (m = path.match(/^\/posts\/(\d{1,12})\/cancel$/))) return await ok(cancel(Number(m[1]), body, headers));
      if (route === "PUT /shop") {
        needGm(headers);
        if (body.id === null) {
          await store.putShop(null);
          return await ok({ ok: true });
        }
        return await ok(putShop(body));
      }
      if (route === "PUT /stock") {
        needGm(headers);
        return await ok(putStock(body));
      }
      if (route === "GET /gm") {
        needGm(headers);
        // What the server holds, so a GM tool resends what is missing (a new or emptied server).
        const snap = await store.snapshot();
        return await ok({ ok: true, seq: snap.seq, shop: snap.shop ? snap.shop.id : null, stocked: !!snap.avail, journal: snap.gm || 0 });
      }
      if (route === "GET /gm/journal") {
        needGm(headers);
        const after = Number((req.query || {}).after || 0);
        if (!isInt(after, 0, 1e12)) throw new HttpError(400, "Paramètre after invalide.");
        const r = await store.gmJournal({ after, limit: 300 });
        return await ok({ ok: true, ...r, more: r.events.length === 300 });
      }
      if (route === "POST /gm/journal") {
        needGm(headers);
        return await ok(gmAppend(body));
      }
      throw new HttpError(404, "Route inconnue.");
    } catch (e) {
      if (e instanceof HttpError) return { status: e.status, body: { ok: false, error: e.message, ...e.extra } };
      console.error(e);
      return { status: 500, body: { ok: false, error: "Erreur du serveur du Bazar." } };
    }
  };
}
