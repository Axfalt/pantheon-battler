// The Bazar store in memory: for the local test server and the tests. Same rules as the
// DynamoDB store (store-dynamo.mjs): every write checks the sequence number it was computed
// from, a slot is bought once, the copies available never go below 0, an account is saved over
// the version it was read at, a name belongs to one account.
//
// Every item expires `ttlDays` after its last write, like DynamoDB's TTL (there, the deletion
// itself can come up to a few days later; reads here skip expired items right away).
//
// With `file`, the state is saved there after each write and read back at start.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { Conflict } from "./handler.mjs";

export function memoryStore({ file = null, ttlDays = 60, clock = () => Date.now() } = {}) {
  const fresh = () => ({ seq: 0, shop: null, stock: null, avail: null, slots: {}, events: [], accounts: {}, posts: {}, sessions: {}, names: {}, mh: {} });
  let st = fresh();
  if (file && existsSync(file)) st = { ...st, ...JSON.parse(readFileSync(file, "utf8")) };
  const save = () => file && writeFileSync(file, JSON.stringify(st, null, 1));
  const copy = (x) => (x == null ? x : structuredClone(x));
  const slotKey = (shop, level, slot) => `${shop}#${level}#${slot}`;
  const ttl = () => Math.floor(clock() / 1000) + ttlDays * 86400;
  const alive = (item) => (item && (!item.ttl || item.ttl > clock() / 1000) ? item : null);

  // The checks of an account write (throw before anything changes), then the write itself.
  function checkAccount(w) {
    if (!w) return;
    const cur = alive(st.accounts[w.item.id]);
    if (w.expectVersion == null ? cur : !cur || cur.v !== w.expectVersion) throw new Conflict("account");
    if (w.claim) {
      const owner = alive(st.names[w.claim.name]);
      if (owner && owner.account !== w.item.id && !(w.claim.takeover && !owner.verified)) throw new Conflict("name");
    }
    if (w.release && alive(st.names[w.release])?.account !== w.item.id) throw new Conflict("name");
    if (w.mh) {
      const link = alive(st.mh[w.mh]);
      if (link && link.account !== w.item.id) throw new Conflict("mh");
    }
  }
  function writeAccount(w) {
    if (!w) return;
    const t = ttl();
    st.accounts[w.item.id] = { a: copy(w.item), v: w.item.version, ttl: t };
    if (w.claim) st.names[w.claim.name] = { account: w.item.id, verified: w.claim.verified, ttl: t };
    if (w.release) delete st.names[w.release];
    if (w.session) st.sessions[w.session] = { account: w.item.id, epoch: w.item.epoch || 0, ttl: t };
    if (w.posts !== undefined) st.posts[w.item.id] = { posts: copy(w.posts), ttl: t };
    if (w.mh) st.mh[w.mh] = { account: w.item.id, ttl: t };
  }

  return {
    async snapshot() {
      return { seq: st.seq, shop: copy(st.shop), avail: copy(st.avail) };
    },
    async slots(shopId) {
      return Object.entries(st.slots)
        .filter(([k]) => k.startsWith(shopId + "#"))
        .map(([, v]) => copy(v));
    },
    async events({ after = null, limit = 500, last = null } = {}) {
      if (last != null) return copy(st.events.slice(-last).reverse());
      return copy(st.events.filter((e) => e.seq > after).slice(0, limit));
    },
    async event(seq) {
      return copy(st.events.find((e) => e.seq === seq) || null);
    },
    async commit({ expectSeq, event, slot = null, shopId = null, freeSlot = null, cancels = null, delta = null, account = null }) {
      if (st.seq !== expectSeq) throw new Conflict("seq");
      if (shopId && (!st.shop || st.shop.id !== shopId)) throw new Conflict("shop");
      if (slot) {
        const taken = st.slots[slotKey(slot.shop, slot.level, slot.slot)];
        if (taken) throw new Conflict("slot", { player: taken.player });
      }
      if (cancels != null && st.events.find((e) => e.seq === cancels)?.cancelled) throw new Conflict("cancelled");
      if (delta && st.avail) {
        for (const [id, n] of Object.entries(delta)) if (n < 0 && (st.avail[id] ?? 0) + n < 0) throw new Conflict("stock", { unit: id });
      }
      checkAccount(account);
      // All checks passed: apply everything.
      st.seq = event.seq;
      st.events.push({ ...copy(event), ttl: ttl() });
      if (slot) st.slots[slotKey(slot.shop, slot.level, slot.slot)] = { ...copy(slot), ttl: ttl() };
      if (freeSlot) {
        const k = slotKey(freeSlot.shop, freeSlot.level, freeSlot.slot);
        if (st.slots[k] && st.slots[k].seq === freeSlot.seq) delete st.slots[k];
      }
      if (cancels != null) {
        const t = st.events.find((e) => e.seq === cancels);
        t.cancelled = { seq: event.seq, at: event.at, by: event.by };
      }
      if (delta && st.avail) for (const [id, n] of Object.entries(delta)) st.avail[id] = (st.avail[id] ?? 0) + n;
      writeAccount(account);
      save();
    },
    async putShop(shop) {
      st.shop = copy(shop);
      save();
    },
    async putStock({ expectSeq, stock, avail }) {
      if (st.seq !== expectSeq) throw new Conflict("seq");
      st.stock = copy(stock);
      st.avail = copy(avail);
      save();
    },

    // Accounts.
    async account(id) {
      const item = alive(st.accounts[id]);
      return item ? copy(item.a) : null;
    },
    async session(hash) {
      const item = alive(st.sessions[hash]);
      return item ? { account: item.account, epoch: item.epoch || 0 } : null;
    },
    async posts(id) {
      const item = alive(st.posts[id]);
      return item ? copy(item.posts) : [];
    },
    async nameOwner(name) {
      const item = alive(st.names[name]);
      return item ? { account: item.account, verified: item.verified } : null;
    },
    async mhAccount(mhId) {
      const item = alive(st.mh[mhId]);
      return item ? item.account : null;
    },
    async saveAccount(w) {
      checkAccount(w);
      writeAccount(w);
      save();
    },
    async putSession(hash, accountId, epoch = 0) {
      st.sessions[hash] = { account: accountId, epoch, ttl: ttl() };
      save();
    },
    async dropSession(hash) {
      delete st.sessions[hash];
      save();
    },
  };
}
