// node --test server/handler.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { createHandler } from "./handler.mjs";
import { memoryStore } from "./store-memory.mjs";
import { fakeMyHordes } from "./myhordes.mjs";

const GM = { authorization: "Bearer secret-key" };
const DAY = 86400 * 1000;

function setup({ myHordes = fakeMyHordes() } = {}) {
  let clock = 1_800_000_000_000;
  const store = memoryStore({ clock: () => clock });
  const handle = createHandler(store, { gmKey: "secret-key", now: () => clock, myHordes });
  const call = async (method, path, body, headers = {}, query = {}) => handle({ method, path, query, headers, body: body ? JSON.stringify(body) : "" });
  return { store, call, later: (days) => (clock += days * DAY) };
}

async function open(call, { counts = { boar: 2, hydra: 1, medusa: 3 } } = {}) {
  assert.equal((await call("PUT", "/stock", { counts, through: 0 }, GM)).status, 200);
  const shop = { id: "dev.1", page: 3, shops: [{ level: 1, units: ["boar", "boar", "medusa"] }, { level: 2, units: ["hydra", "boar"] }] };
  assert.equal((await call("PUT", "/shop", shop, GM)).status, 200);
  return shop;
}

const god = (name, favors = 10) => ({ name, favors, level: 1, invested: 0, board: [], reserve: [] });
const boar = { id: "boar", star: false, atk: 1, hp: 4, def: 0, deaths: 0 };

// A player without MyHordes: their account is made with their name.
async function player(call, name, state = god(name)) {
  const r = await call("POST", "/accounts", { name, state });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const p = { name, token: r.body.token, version: r.body.version, auth: { authorization: `Bearer ${r.body.token}` } };
  // Confirms a post; the god after it gets one more boar and 2🙏 less.
  p.post = async (buy, extra = {}) => {
    const after = { ...god(name, 8), board: [boar] };
    const r = await call("POST", "/posts", { version: p.version, shop: "dev.1", buy, returned: [], forum: "acheter", fee: 1, price: 1, after, ...extra }, p.auth);
    if (r.status === 200) p.version = r.body.version;
    return r;
  };
  p.cancel = async (seq) => {
    const r = await call("POST", `/posts/${seq}/cancel`, {}, p.auth);
    if (r.status === 200 && r.body.version) p.version = r.body.version;
    return r;
  };
  return p;
}

test("one buyer per slot", async () => {
  const { call } = setup();
  await open(call);
  const alice = await player(call, "Alice");
  const bob = await player(call, "Bob");
  const a = await alice.post({ level: 1, slot: 0, unit: "boar" });
  assert.equal(a.status, 200, JSON.stringify(a.body));
  assert.equal(a.body.event.player, "Alice");
  assert.deepEqual(a.body.state.board, [boar]);
  const b = await bob.post({ level: 1, slot: 0, unit: "boar" });
  assert.equal(b.status, 409);
  assert.match(b.body.error, /Alice/);
  const view = (await call("GET", "/bazar")).body;
  assert.equal(view.avail.boar, 1);
  assert.deepEqual(view.slots.map((s) => [s.level, s.slot, s.player]), [[1, 0, "Alice"]]);
});

test("the last copy is shared between the level shops", async () => {
  const { call } = setup();
  await open(call, { counts: { boar: 1, hydra: 1, medusa: 3 } });
  assert.equal((await (await player(call, "Alice")).post({ level: 2, slot: 1, unit: "boar" })).status, 200);
  const b = await (await player(call, "Bob")).post({ level: 1, slot: 1, unit: "boar" });
  assert.equal(b.status, 409);
  assert.match(b.body.error, /plus d'exemplaire/);
});

test("wrong slot, wrong shop, no account", async () => {
  const { call } = setup();
  await open(call);
  const alice = await player(call, "Alice");
  assert.equal((await alice.post({ level: 1, slot: 2, unit: "boar" })).status, 409);
  assert.equal((await alice.post({ level: 1, slot: 0, unit: "boar" }, { shop: "old" })).status, 409);
  const anonymous = await call("POST", "/posts", { version: 1, shop: "dev.1", buy: null, forum: "passer", after: god("X") });
  assert.equal(anonymous.status, 401);
});

test("cancel frees the slot, gives the copy back and restores the god; only its player or a GM", async () => {
  const { call } = setup();
  await open(call);
  const alice = await player(call, "Alice");
  const bob = await player(call, "Bob");
  const a = (await alice.post({ level: 1, slot: 0, unit: "boar" })).body.event;
  assert.equal((await bob.cancel(a.seq)).status, 403);
  const c = await alice.cancel(a.seq);
  assert.equal(c.status, 200, JSON.stringify(c.body));
  assert.equal(c.body.restored, true);
  assert.deepEqual(c.body.state, god("Alice"));
  assert.equal(c.body.posts[0].cancelled.by, "Alice");
  assert.equal((await alice.cancel(a.seq)).status, 409);
  const view = (await call("GET", "/bazar")).body;
  assert.equal(view.avail.boar, 2);
  assert.equal(view.slots.length, 0);
  assert.ok(view.events.find((e) => e.seq === a.seq).cancelled);
  // A GM cancels Bob's post: Bob's god goes back too, he sees it in GET /me.
  const b = (await bob.post({ level: 1, slot: 0, unit: "boar" })).body.event;
  const g = await call("POST", `/posts/${b.seq}/cancel`, { by: "Zeus" }, GM);
  assert.equal(g.status, 200);
  const me = (await call("GET", "/me", null, bob.auth)).body;
  assert.deepEqual(me.state, god("Bob"));
  assert.deepEqual([me.posts[0].cancelled.by, me.posts[0].cancelled.gm], ["Zeus", true]);
});

test("a god changed after a post is not restored by its cancellation", async () => {
  const { call } = setup();
  await open(call);
  const alice = await player(call, "Alice");
  const a = (await alice.post({ level: 1, slot: 0, unit: "boar" })).body.event;
  const edited = { ...god("Alice", 8), board: [boar, boar] }; // the GM's merge reward added by hand
  const put = await call("PUT", "/me", { version: alice.version, state: edited }, alice.auth);
  assert.equal(put.status, 200, JSON.stringify(put.body));
  alice.version = put.body.version;
  const c = await alice.cancel(a.seq);
  assert.equal(c.body.restored, false);
  assert.deepEqual(c.body.state, edited);
});

test("a sale returns copies; cancelling it is refused once they are bought again", async () => {
  const { call } = setup();
  await open(call, { counts: { boar: 0, hydra: 1, medusa: 3 } });
  const alice = await player(call, "Alice", { ...god("Alice"), board: [boar] });
  const s = (await alice.post(null, { returned: ["boar"], forum: "passer · vendre Sanglier", price: 0, sales: 1, after: god("Alice", 10) })).body.event;
  assert.equal((await call("GET", "/bazar")).body.avail.boar, 1);
  assert.equal((await (await player(call, "Bob")).post({ level: 1, slot: 0, unit: "boar" })).status, 200);
  const c = await alice.cancel(s.seq);
  assert.equal(c.status, 409);
  assert.match(c.body.error, /rachetée/);
});

test("the GM reserve and the events it doesn't include yet", async () => {
  const { call } = setup();
  await open(call);
  await (await player(call, "Alice")).post({ level: 1, slot: 0, unit: "boar" }); // seq 1
  await (await player(call, "Bob")).post({ level: 1, slot: 2, unit: "medusa" }); // seq 2
  // The GM imported seq 1 only (boar 2 → 1) and added a hydra by hand.
  const r = await call("PUT", "/stock", { counts: { boar: 1, hydra: 2, medusa: 3 }, through: 1 }, GM);
  assert.deepEqual(r.body.avail, { boar: 1, hydra: 2, medusa: 2 });
  const ev = (await call("GET", "/events", null, {}, { after: "1" })).body.events;
  assert.deepEqual(ev.map((e) => e.seq), [2]);
});

test("GM routes need the key", async () => {
  const { call } = setup();
  assert.equal((await call("PUT", "/shop", { id: "x", page: 1, shops: [] })).status, 401);
  assert.equal((await call("PUT", "/stock", { counts: {}, through: 0 }, { authorization: "Bearer nope" })).status, 401);
  assert.equal((await call("GET", "/gm", null, GM)).status, 200);
});

test("GM journal: numbered in arrival order, each id once, GM key only, 60 days", async () => {
  const { call, later } = setup();
  const ev = (id, extra = {}) => ({ id, at: 1, by: "MJ", type: "reset", players: 6, ...extra });
  assert.equal((await call("POST", "/gm/journal", { events: [ev("a.1")] })).status, 401);
  assert.equal((await call("GET", "/gm/journal", null, {}, { after: "0" })).status, 401);
  let r = await call("POST", "/gm/journal", { events: [ev("a.1"), ev("a.2")] }, GM);
  assert.deepEqual([r.status, r.body.added, r.body.last], [200, 2, 2]);
  // A resend (lost answer) and another GM's event: only the new one is added.
  r = await call("POST", "/gm/journal", { events: [ev("a.2"), ev("b.1", { type: "move" })] }, GM);
  assert.deepEqual([r.body.added, r.body.last], [1, 3]);
  r = await call("GET", "/gm/journal", null, GM, { after: "1" });
  assert.deepEqual(r.body.events.map((x) => [x.n, x.e.id]), [[2, "a.2"], [3, "b.1"]]);
  assert.equal(r.body.last, 3);
  assert.equal((await call("GET", "/gm", null, GM)).body.journal, 3);
  // Bad input.
  assert.equal((await call("POST", "/gm/journal", { events: [{ at: 1 }] }, GM)).status, 400);
  assert.equal((await call("POST", "/gm/journal", { events: [ev("c.1"), ev("c.1")] }, GM)).status, 400);
  assert.equal((await call("POST", "/gm/journal", { events: Array.from({ length: 26 }, (_, k) => ev(`d.${k}`)) }, GM)).status, 400);
  assert.equal((await call("POST", "/gm/journal", { events: [ev("e.1", { forum: "x".repeat(17000) })] }, GM)).status, 413);
  // 60 days after its last write, the journal is gone and numbering starts again.
  later(59);
  assert.equal((await call("GET", "/gm/journal", null, GM, { after: "0" })).body.events.length, 3);
  later(2);
  r = await call("GET", "/gm/journal", null, GM, { after: "0" });
  assert.deepEqual([r.body.events.length, r.body.last], [0, 0]);
  r = await call("POST", "/gm/journal", { events: [ev("a.1")] }, GM);
  assert.deepEqual([r.body.added, r.body.last], [1, 1]);
});

test("polls: only the newer events after `since`", async () => {
  const { call } = setup();
  await open(call);
  const alice = await player(call, "Alice");
  const a = (await alice.post({ level: 1, slot: 0, unit: "boar" })).body.event;
  const first = (await call("GET", "/bazar")).body;
  assert.equal(first.since, null);
  assert.equal(first.myhordes, true);
  assert.deepEqual(first.events.map((e) => e.seq), [a.seq]);
  const idle = (await call("GET", "/bazar", null, {}, { since: String(first.seq) })).body;
  assert.equal(idle.since, first.seq);
  assert.deepEqual(idle.events, []);
  const b = (await (await player(call, "Bob")).post({ level: 1, slot: 1, unit: "boar" })).body.event;
  await alice.cancel(a.seq);
  const next = (await call("GET", "/bazar", null, {}, { since: String(first.seq) })).body;
  assert.deepEqual(next.events.map((e) => [e.seq, e.type]), [[b.seq, "post"], [b.seq + 1, "cancel"]]);
  // A page further ahead than the server (a reset server) gets the full list again.
  assert.equal((await call("GET", "/bazar", null, {}, { since: "99" })).body.since, null);
});

test("accounts: a name belongs to one account; a code opens it on another device", async () => {
  const { call } = setup();
  const alice = await player(call, "Alice");
  const twin = await call("POST", "/accounts", { name: "alice", state: god("alice") });
  assert.equal(twin.status, 409);
  assert.match(twin.body.error, /déjà pris/);
  const code = (await call("POST", "/sessions", {}, alice.auth)).body.token;
  const other = (await call("GET", "/me", null, { authorization: `Bearer ${code}` })).body;
  assert.equal(other.account.name, "Alice");
  // Two devices: the second one's save over an old version is refused.
  const one = await call("PUT", "/me", { version: other.version, state: { ...god("Alice", 3) } }, alice.auth);
  assert.equal(one.status, 200);
  const two = await call("PUT", "/me", { version: other.version, state: { ...god("Alice", 5) } }, { authorization: `Bearer ${code}` });
  assert.equal(two.status, 409);
  assert.equal(two.body.stale, true);
  // Renaming frees the old name.
  const renamed = await call("PUT", "/me", { version: one.body.version, name: "Alicia" }, alice.auth);
  assert.equal(renamed.body.account.name, "Alicia");
  assert.equal(renamed.body.state.name, "Alicia");
  assert.equal((await call("POST", "/accounts", { name: "Alice" })).status, 200);
  // Logging out ends that device's session only.
  await call("DELETE", "/sessions", null, { authorization: `Bearer ${code}` });
  assert.equal((await call("GET", "/me", null, { authorization: `Bearer ${code}` })).status, 401);
  assert.equal((await call("GET", "/me", null, alice.auth)).status, 200);
});

test("MyHordes login: proves the name, takes it from a keyless god, links this browser's god", async () => {
  const { call } = setup();
  await open(call);
  // Mallory took the name Bob without a key.
  const mallory = await player(call, "Bob");
  // The real Bob logs in with MyHordes: the name is his now.
  const bob = await call("POST", "/login/myhordes", { key: "test-Bob", state: god("Bob", 4) });
  assert.equal(bob.status, 200, JSON.stringify(bob.body));
  assert.deepEqual(bob.body.account.name, "Bob");
  assert.equal(bob.body.account.verified, true);
  assert.equal(bob.body.state.favors, 4);
  const m = await mallory.post({ level: 1, slot: 0, unit: "boar" });
  assert.equal(m.status, 409);
  assert.match(m.body.error, /vérifié par MyHordes/);
  // Bob on another device: same account, his god as saved.
  const again = await call("POST", "/login/myhordes", { key: "test-Bob", state: god("Bob", 99) });
  assert.equal(again.body.account.id, bob.body.account.id);
  assert.equal(again.body.state.favors, 4);
  // Carol plays without a key, then links MyHordes: same god, now verified.
  const carol = await player(call, "Carol");
  await carol.post({ level: 1, slot: 1, unit: "boar" });
  const linked = await call("POST", "/login/myhordes", { key: "test-Carol" }, carol.auth);
  assert.equal(linked.body.account.id, (await call("GET", "/me", null, carol.auth)).body.account.id);
  assert.equal(linked.body.account.verified, true);
  assert.equal(linked.body.posts.length, 1);
  // A wrong key, and a server without a MyHordes app key.
  assert.equal((await call("POST", "/login/myhordes", { key: "nope" })).status, 401);
  const bare = setup({ myHordes: null });
  assert.equal((await bare.call("POST", "/login/myhordes", { key: "test-Bob" })).status, 501);
  assert.equal((await bare.call("GET", "/bazar")).body.myhordes, false);
});

test("everything left untouched for 60 days expires", async () => {
  const { call, later } = setup();
  const alice = await player(call, "Alice");
  later(59);
  assert.equal((await call("GET", "/me", null, alice.auth)).status, 200);
  const put = await call("PUT", "/me", { version: alice.version, state: god("Alice", 1) }, alice.auth); // renews the 60 days
  assert.equal(put.status, 200);
  later(59);
  assert.equal((await call("GET", "/me", null, alice.auth)).status, 200);
  later(2);
  assert.equal((await call("GET", "/me", null, alice.auth)).status, 401);
  assert.equal((await call("POST", "/accounts", { name: "Alice" })).status, 200); // the name is free again
});

test("history and devices: posts with their text, expiry date, logging the other devices out", async () => {
  const { call, later } = setup();
  await open(call);
  const alice = await player(call, "Alice");
  await alice.post({ level: 1, slot: 0, unit: "boar" }, { forum: "[b]acheter Sanglier[/b]\nTerrain : 🐗 Sanglier 1/4/0" });
  const a2 = (await alice.post(null, { forum: "[b]passer[/b]", price: 0 })).body.event;
  await alice.cancel(a2.seq);
  later(3);
  const me = (await call("GET", "/me", null, alice.auth)).body;
  assert.deepEqual(me.posts.map((p) => [p.seq, p.forum.split("\n")[0], !!p.cancelled]), [[2, "[b]passer[/b]", true], [1, "[b]acheter Sanglier[/b]", false]]);
  assert.equal(me.account.expires, me.account.updated + 60 * DAY);
  // A plain save of the god doesn't send the posts back (they didn't change).
  const put = (await call("PUT", "/me", { version: me.version, state: god("Alice", 3) }, alice.auth)).body;
  assert.equal(put.posts, undefined);
  // Two other devices; "log the others out" keeps only this one.
  const codes = [];
  for (let i = 0; i < 2; i++) codes.push((await call("POST", "/sessions", {}, alice.auth)).body.token);
  const drop = await call("DELETE", "/sessions/others", null, { authorization: `Bearer ${codes[0]}` });
  assert.equal(drop.status, 200, JSON.stringify(drop.body));
  assert.equal((await call("GET", "/me", null, { authorization: `Bearer ${codes[0]}` })).status, 200);
  assert.equal((await call("GET", "/me", null, { authorization: `Bearer ${codes[1]}` })).status, 401);
  assert.equal((await call("GET", "/me", null, alice.auth)).status, 401);
  // A code made afterwards works.
  const fresh = (await call("POST", "/sessions", {}, { authorization: `Bearer ${codes[0]}` })).body.token;
  assert.equal((await call("GET", "/me", null, { authorization: `Bearer ${fresh}` })).status, 200);
});
