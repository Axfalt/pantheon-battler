// Pantheon Battler: the GM tools (/gm_tool/). The Bazar draw, the common reserve and its journal.
//
// Everything a GM does is an event appended to the journal; the reserve is recomputed by
// replaying the events in time order.
//
// With the Bazar server (Réglages tab), the journal is kept there and shared by every GM: each
// action waits in this browser until the server has it (`pending`), and the server's journal is
// read back incrementally (`remote`, cached here). The current shop and the reserve are published
// for the players' Bazar tab, and the posts players confirm come back as journal events whose id
// is "srv.<server number>" (the server keeps each id once, so two GMs importing the same post is
// harmless). Without a server, the journal is kept in this browser only. Either way it can be
// saved to a file and imported back.
"use strict";

(function () {
  const { $, $$, el, Engine, store, copyText, messages, renderTwino, unitById, tipBody, placeTip } = window.PB;

  const SERVER_POLL_MS = 10000; // the Bazar server: players' posts
  const MAX_COPIES = 999;
  const JOURNAL = "pantheon.gm.journal.local";

  // ---------------------------------------------------------------- Settings and journal
  // settings: { name }
  let settings = store.get("pantheon.gm.settings", {}) || {};
  let device = store.get("pantheon.gm.device", null);
  if (typeof device !== "string" || !/^[a-z0-9]{6,12}$/.test(device)) {
    device = Array.from(crypto.getRandomValues(new Uint8Array(6)), (b) => (b % 36).toString(36)).join("");
    store.set("pantheon.gm.device", device);
  }

  let mine = []; // without a server: the journal, append-only
  let remote = { last: 0, events: [] }; // with a server: its journal as last read (number, events)
  let pending = []; // with a server: this browser's actions it doesn't have yet

  // Events of several journals, each id once, in time order.
  const byTime = (a, b) => a.at - b.at || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  function mergeEvents(...lists) {
    const seen = new Set();
    const out = [];
    for (const list of lists)
      for (const e of Array.isArray(list) ? list : [])
        if (e && typeof e.id === "string" && !seen.has(e.id)) {
          seen.add(e.id);
          out.push(e);
        }
    return out.sort(byTime);
  }

  // The reserve used to be shared through a GitHub Gist: its journals, kept by this browser,
  // become the local journal (once), and the GitHub token is forgotten.
  function migrateGist() {
    const gist = settings.gist;
    if (!gist) return;
    const others = store.get(`pantheon.gm.others.${gist}`, {}) || {};
    const old = store.get(JOURNAL, []);
    const events = mergeEvents(store.get(`pantheon.gm.journal.${gist}`, []), ...Object.values(others).map((j) => j && j.events));
    if (events.length) {
      if (Array.isArray(old) && old.length) store.set("pantheon.gm.journal.before-gist", old);
      store.set(JOURNAL, events);
    }
    for (const k of ["journal", "synced", "others"]) {
      try {
        localStorage.removeItem(`pantheon.gm.${k}.${gist}`);
      } catch (e) {
        /* ignore */
      }
    }
    settings = { name: settings.name };
    saveSettings();
  }

  // With a server, its journal and what waits for it are kept per server address.
  const online = () => serverOn();
  const remoteKey = () => `pantheon.gm.remote.${server.url}`;
  const pendingKey = () => `pantheon.gm.pending.${server.url}`;
  const allEvents = () => (online() ? mergeEvents(remote.events, pending) : mine);
  const readRemote = () => {
    const r = store.get(remoteKey(), null);
    return r && Number.isInteger(r.last) && Array.isArray(r.events) ? r : null;
  };

  function loadJournal() {
    mine = store.get(JOURNAL, []);
    if (!Array.isArray(mine)) mine = [];
    remote = (online() && readRemote()) || { last: 0, events: [] };
    pending = online() ? store.get(pendingKey(), []) : [];
    if (!Array.isArray(pending)) pending = [];
  }
  function saveJournal() {
    if (!online()) return store.set(JOURNAL, mine);
    store.set(remoteKey(), remote);
    store.set(pendingKey(), pending);
  }
  const saveSettings = () => store.set("pantheon.gm.settings", settings);
  const gmName = () => (settings.name || "").trim() || "MJ";

  // Another tab of this browser may have written to the journal meanwhile.
  function reloadMine() {
    if (!online()) {
      const stored = store.get(JOURNAL, []);
      if (Array.isArray(stored) && stored.length > mine.length) mine = stored;
      return;
    }
    const r = readRemote();
    if (r && r.last > remote.last) remote = r;
    const known = new Set(remote.events.map((e) => e.id));
    pending = mergeEvents(pending, store.get(pendingKey(), [])).filter((e) => !known.has(e.id));
  }

  // Records an action: replayed at once, sent to the Bazar server soon after.
  function record(type, fields, fixed = null) {
    reloadMine();
    const e = { id: `${device}.${Date.now().toString(36)}.${allEvents().length.toString(36)}`, at: Date.now(), by: gmName(), type, ...fields, ...(fixed || {}) };
    if (online()) pending.push(e);
    else mine.push(e);
    saveJournal();
    refresh();
    return e;
  }

  // Adds events (an imported file), each id once; returns how many were new.
  function addEvents(list) {
    reloadMine();
    const known = new Set(allEvents().map((e) => e.id));
    const fresh = mergeEvents(list).filter((e) => !known.has(e.id));
    if (!fresh.length) return 0;
    if (online()) pending = mergeEvents(pending, fresh);
    else mine = mergeEvents(mine, fresh);
    saveJournal();
    refresh();
    return fresh.length;
  }

  // ---------------------------------------------------------------- Replay
  const poolCopies = (tier, players) => {
    const r = Engine.info.rules;
    return Math.max(r.merge_copies, Math.round(r.pool_per_player[tier - 1] * players));
  };
  const clampCopies = (n) => Math.max(0, Math.min(MAX_COPIES, Math.round(n)));
  const fullStock = (players) => Object.fromEntries(Engine.info.roster.map((u) => [u.id, poolCopies(u.tier, players)]));
  const validPlayers = (n) => Number.isInteger(n) && n >= 1 && n <= 200;

  function validEvent(e) {
    if (!e || typeof e !== "object" || typeof e.id !== "string" || !Number.isFinite(e.at)) return false;
    switch (e.type) {
      case "reset":
      case "players":
        return validPlayers(e.players);
      case "move":
        return !!unitById(e.unit) && Number.isInteger(e.delta) && Math.abs(e.delta) <= MAX_COPIES;
      case "set":
        return !!unitById(e.unit) && Number.isInteger(e.count) && e.count >= 0 && e.count <= MAX_COPIES;
      case "shop":
        return Number.isInteger(e.page) && Array.isArray(e.shops) && typeof e.forum === "string";
      case "undo":
        return typeof e.target === "string";
      case "player":
        return (
          Number.isInteger(e.srv) &&
          typeof e.player === "string" &&
          (e.taken == null || !!unitById(e.taken)) &&
          Array.isArray(e.returned) &&
          e.returned.every((id) => !!unitById(id))
        );
      default:
        return false;
    }
  }

  let view = null; // the last replay

  function replay() {
    const all = mergeEvents(allEvents().filter(validEvent));
    // An event counts unless an undo that counts targets it (an undo can be undone too).
    const undosOf = new Map();
    for (const e of all) if (e.type === "undo") undosOf.set(e.target, [...(undosOf.get(e.target) || []), e]);
    const memo = new Map();
    const counts = (e) => {
      if (!memo.has(e.id)) memo.set(e.id, !(undosOf.get(e.id) || []).some(counts));
      return memo.get(e.id);
    };
    let stock = null;
    let players = null;
    let shop = null;
    let maxPage = 0;
    const after = new Map(); // event id → copies of its unit left after it
    const pages = new Map(); // page → shop events
    for (const e of all) {
      if (e.type === "undo" || !counts(e)) continue;
      if (e.type === "reset") {
        players = e.players;
        stock = fullStock(players);
      } else if (e.type === "players" && stock) {
        for (const u of Engine.info.roster) stock[u.id] = clampCopies(stock[u.id] + poolCopies(u.tier, e.players) - poolCopies(u.tier, players));
        players = e.players;
      } else if (e.type === "move" && stock) {
        stock[e.unit] = clampCopies(stock[e.unit] + e.delta);
        after.set(e.id, stock[e.unit]);
      } else if (e.type === "set" && stock) {
        stock[e.unit] = e.count;
        after.set(e.id, e.count);
      } else if (e.type === "player" && stock) {
        // A post confirmed by a player on the Bazar server.
        if (e.taken) stock[e.taken] = clampCopies(stock[e.taken] - 1);
        for (const id of e.returned) stock[id] = clampCopies(stock[id] + 1);
      } else if (e.type === "shop") {
        shop = e;
        maxPage = Math.max(maxPage, e.page);
        pages.set(e.page, [...(pages.get(e.page) || []), e]);
      }
    }
    // The last server event this journal includes (posts and their cancellations).
    const srvSeen = all.reduce((m, e) => (Number.isInteger(e.srv) ? Math.max(m, e.srv) : m), 0);
    view = { all, counts, stock, players, shop, nextPage: maxPage + 1, after, pages, srvSeen };
    return view;
  }

  // ---------------------------------------------------------------- Backup
  // The journal can be saved to a file and imported back (another browser, another server).
  // Importing merges: an event already there is skipped.
  function exportJournal() {
    const day = new Date().toLocaleDateString("sv-SE"); // YYYY-MM-DD, local
    const events = allEvents();
    const blob = new Blob([JSON.stringify({ app: "pantheon-battler", kind: "gm-journal", name: gmName(), saved: new Date().toISOString(), events }, null, 1)], {
      type: "application/json",
    });
    const a = el("a", { href: URL.createObjectURL(blob), download: `pantheon-journal-${day}.json` });
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
    messages($("#sh-backup-messages"), [], [], [`Journal enregistré (${events.length} action${events.length > 1 ? "s" : ""}).`]);
  }

  async function importJournal(input) {
    const files = [...input.files];
    input.value = "";
    if (!files.length) return;
    const lists = [];
    try {
      for (const f of files) {
        const j = JSON.parse(await f.text());
        const events = Array.isArray(j) ? j : j && j.events;
        if (!Array.isArray(events)) throw new Error(`${f.name} n'est pas un journal de Pantheon Battler.`);
        lists.push(events.filter(validEvent));
      }
    } catch (e) {
      return messages($("#sh-backup-messages"), [e instanceof SyntaxError ? "Fichier illisible (JSON attendu)." : e.message]);
    }
    const known = new Set(allEvents().map((e) => e.id));
    const count = new Set(lists.flat().map((e) => e.id).filter((id) => !known.has(id))).size;
    if (!count) return messages($("#sh-backup-messages"), [], [], ["Rien de nouveau : le journal contient déjà toutes ces actions."]);
    const where = online() ? "au journal (sur le serveur du Bazar, pour tous les MJ)" : "au journal de ce navigateur";
    if (!window.confirm(`Ajouter ${count} action${count > 1 ? "s" : ""} ${where} ? La réserve est recalculée et publiée.`)) return;
    const added = addEvents(lists.flat());
    messages($("#sh-backup-messages"), [], [], [`${added} action${added > 1 ? "s" : ""} importée${added > 1 ? "s" : ""}.`]);
  }

  // ---------------------------------------------------------------- Bazar server
  // server: { url, key } — the GM key stays in this browser.
  const local = /^(localhost|127\.0\.0\.1|\[::1\])$/.test(location.hostname);
  let server = store.get("pantheon.gm.server", null) || (local ? { url: `${location.origin}/api`, key: "dev" } : {});
  const serverOn = () => !!(server.url && server.key);
  const pushedKey = () => `pantheon.gm.pushed.${server.url}`;
  let pushed = {}; // what this browser last sent: { shop: id, stock: json }
  let srvError = null;
  let srvLast = null;

  class ServerError extends Error {}

  async function api(method, path, body) {
    let r;
    try {
      r = await fetch(server.url.replace(/\/+$/, "") + path, {
        method,
        cache: "no-store",
        headers: { Authorization: `Bearer ${server.key}`, ...(body ? { "Content-Type": "application/json" } : {}) },
        body: body ? JSON.stringify(body) : undefined,
      });
    } catch (e) {
      throw new ServerError("Le serveur du Bazar ne répond pas.");
    }
    let j = null;
    try {
      j = await r.json();
    } catch (e) {
      /* not JSON */
    }
    if (!r.ok) throw new ServerError((j && j.error) || `Le serveur du Bazar a répondu ${r.status}.`);
    return j;
  }

  const unitList = (ids) =>
    ids
      .map((id) => unitById(id))
      .filter(Boolean)
      .map((u) => `${u.emoji} ${u.name}`)
      .join(", ");

  // The posts and cancellations confirmed since the last one this journal has.
  async function importEvents() {
    let after = view.srvSeen;
    for (;;) {
      const r = await api("GET", `/events?after=${after}`);
      for (const e of r.events) {
        after = e.seq;
        if (view.all.some((x) => x.id === `srv.${e.seq}`)) continue;
        const fixed = { id: `srv.${e.seq}`, at: e.at };
        if (e.type === "post") {
          record(
            "player",
            { srv: e.seq, player: e.player, verified: !!e.verified, page: e.page, taken: e.buy ? e.buy.unit : null, returned: e.returned || [] },
            { ...fixed, by: e.player },
          );
        } else if (e.type === "cancel") {
          record("undo", { target: `srv.${e.target}`, srv: e.seq, player: e.player }, { ...fixed, by: e.gm ? `${e.by} (MJ)` : e.player });
        }
      }
      if (!r.more) break;
    }
  }

  async function pushShop() {
    const s = view.shop;
    const id = s ? s.id : null;
    if (pushed.shop === id) return;
    await api(
      "PUT",
      "/shop",
      s
        ? { id: s.id, page: s.page, at: s.at, by: s.by, shops: s.shops.map((x) => ({ level: x.level, units: x.units.map((u) => (typeof u === "string" ? u : u.id)) })) }
        : { id: null },
    );
    pushed.shop = id;
    store.set(pushedKey(), pushed);
  }

  async function pushStock() {
    if (!view.stock) return;
    const body = { counts: view.stock, through: view.srvSeen };
    const json = JSON.stringify(body);
    if (pushed.stock === json) return;
    await api("PUT", "/stock", body);
    pushed.stock = json;
    store.set(pushedKey(), pushed);
  }

  let srvBusy = false;
  let srvAgain = false;
  async function serverSync() {
    if (!serverOn() || !Engine.info || !view) return;
    if (srvBusy) {
      srvAgain = true;
      return;
    }
    srvBusy = true;
    try {
      // A new or emptied server (the local test server forgets everything when restarted): send
      // the shop and the reserve again even if this browser already sent them.
      const held = await api("GET", "/gm");
      if ((held.shop ?? null) !== (pushed.shop ?? null)) delete pushed.shop;
      if (!held.stocked) delete pushed.stock;
      await syncJournal(held);
      refresh({ sync: false });
      // Its numbers start again: importing would mix its new posts with the journal's old ones.
      const reset = held.seq < view.srvSeen;
      if (!reset) await importEvents();
      // Published once the server has every action (the other GMs then publish the same).
      if (!pending.length) {
        await pushShop();
        await pushStock();
      }
      srvError = reset
        ? `Ce serveur du Bazar a été remis à zéro (n° ${held.seq}, le journal en est au n° ${view.srvSeen}) : la boutique et la réserve y sont publiées, mais ses messages ne sont plus importés. Repartez d'un journal vide pour tester.`
        : null;
      srvLast = new Date();
    } catch (e) {
      srvError = e instanceof ServerError ? e.message : `Serveur du Bazar : ${e.message}`;
    } finally {
      srvBusy = false;
      renderServer();
    }
    if (srvAgain) {
      srvAgain = false;
      setTimeout(serverSync, 300);
    }
  }
  // The journal on the server: what waits goes there, what is new comes back.
  async function syncJournal(held) {
    reloadMine();
    const journal = Number(held.journal) || 0;
    if (!readRemote()) {
      // First time with this server: the journal kept in this browser goes there.
      const local = store.get(JOURNAL, []);
      const ask =
        `Le serveur du Bazar a déjà un journal (${journal} action${journal > 1 ? "s" : ""}). Y ajouter les ${local.length} actions ` +
        `gardées dans ce navigateur ?\n\nAnnuler : elles restent dans ce navigateur seulement.`;
      if (Array.isArray(local) && local.length && (journal === 0 || window.confirm(ask))) pending = mergeEvents(pending, local);
      saveJournal();
    } else if (journal < remote.last) {
      // The server's journal was emptied (a new test server, or 60 days without any action):
      // this browser's copy goes back.
      pending = mergeEvents(remote.events, pending);
      remote = { last: 0, events: [] };
      saveJournal();
    }
    // What waits, in pieces the server accepts.
    for (let i = 0; i < pending.length; ) {
      const chunk = [];
      let size = 0;
      while (i < pending.length && chunk.length < 25) {
        const s = JSON.stringify(pending[i]).length;
        if (chunk.length && size + s > 24000) break;
        chunk.push(pending[i]);
        size += s;
        i++;
      }
      await api("POST", "/gm/journal", { events: chunk });
    }
    // What is new: the other GMs' actions, and ours just sent.
    for (;;) {
      const r = await api("GET", `/gm/journal?after=${remote.last}`);
      const have = new Set(remote.events.map((e) => e.id));
      for (const x of r.events) if (x && x.e && typeof x.e.id === "string" && !have.has(x.e.id)) remote.events.push(x.e);
      const lastN = r.events.length ? r.events[r.events.length - 1].n : remote.last;
      remote.last = r.more ? lastN : Math.max(lastN, r.last || 0);
      if (!r.more) break;
    }
    const onServer = new Set(remote.events.map((e) => e.id));
    pending = pending.filter((e) => !onServer.has(e.id));
    saveJournal();
  }

  let srvSoon = null;
  const serverSoon = () => {
    clearTimeout(srvSoon);
    srvSoon = setTimeout(serverSync, 500);
  };

  // A player's post cancelled by a GM: on the server, which sends the cancellation back.
  async function cancelOnServer(e, button) {
    if (!serverOn()) return window.alert("Connectez d'abord le serveur du Bazar (onglet Réglages).");
    if (!window.confirm(`Annuler le message n°${e.srv} de ${e.player} sur le Bazar ?\nL'unité achetée revient en boutique ; le joueur le voit dans son onglet Bazar.`)) return;
    button.disabled = true;
    try {
      await api("POST", `/posts/${e.srv}/cancel`, { by: gmName() });
      await serverSync();
    } catch (err) {
      window.alert(err.message);
      button.disabled = false;
    }
  }

  function renderServer() {
    const box = $("#sh-srv-state");
    if (!box) return;
    const url = $("#sh-srv-url");
    if (document.activeElement !== url) url.value = server.url || "";
    $("#sh-srv-forget").hidden = !serverOn();
    const waiting = pending.length ? ` (${pending.length} action${pending.length > 1 ? "s" : ""} en attente d'envoi)` : "";
    if (!serverOn()) box.textContent = "Pas de serveur : le journal est gardé dans ce navigateur seulement, et les joueurs ne voient pas la boutique dans leur onglet Bazar.";
    else if (srvError) box.textContent = `⚠ ${srvError}${waiting}`;
    else if (!srvLast) box.textContent = "Connexion…";
    else
      box.textContent =
        `Connecté · journal partagé : ${remote.events.length} action${remote.events.length > 1 ? "s" : ""}${waiting} · ` +
        `boutique ${view && view.shop ? `de la page ${view.shop.page}` : "non publiée"} · ` +
        `dernier message reçu n°${view ? view.srvSeen : 0} · à jour ` +
        srvLast.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    box.dataset.state = !serverOn() ? "local" : srvError ? "error" : "ok";
    // The header: where the reserve is published.
    const badge = $("#sync-badge");
    badge.dataset.state = box.dataset.state === "ok" && (!srvLast || pending.length) ? "busy" : box.dataset.state;
    badge.textContent = !serverOn()
      ? "Journal dans ce navigateur seulement · onglet Réglages"
      : srvError
        ? `⚠ Bazar en ligne : voir l'onglet Réglages${waiting}`
        : pending.length
          ? `Envoi au serveur…${waiting}`
          : srvLast
            ? `☁ Journal partagé sur le Bazar en ligne · à jour ${srvLast.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })}`
            : "Connexion au Bazar en ligne…";
  }

  async function saveServer() {
    const url = $("#sh-srv-url").value.trim().replace(/\/+$/, "");
    const key = $("#sh-srv-key").value.trim() || (url === server.url ? server.key : "") || "";
    if (!/^https?:\/\/\S+$/.test(url)) return messages($("#sh-srv-messages"), ["Adresse du serveur invalide (https://…)."]);
    if (!key) return messages($("#sh-srv-messages"), ["Entrez la clé MJ."]);
    const before = server;
    server = { url, key };
    try {
      await api("GET", "/gm");
    } catch (e) {
      server = before;
      return messages($("#sh-srv-messages"), [e.message]);
    }
    store.set("pantheon.gm.server", server);
    pushed = store.get(pushedKey(), {}) || {};
    srvError = null;
    srvLast = null;
    loadJournal();
    refresh({ sync: false });
    $("#sh-srv-key").value = "";
    messages($("#sh-srv-messages"), [], [], ["Serveur du Bazar connecté : le journal y est gardé, la boutique et la réserve y sont publiées."]);
    serverSync();
  }

  function forgetServer() {
    const ask =
      "Déconnecter ce navigateur du serveur du Bazar ? La clé MJ y sera effacée. Le journal (tel qu'il est maintenant) reste dans ce navigateur ; " +
      "les actions des autres MJ n'y arriveront plus.";
    if (!window.confirm(ask)) return;
    reloadMine();
    mine = allEvents();
    store.set(JOURNAL, mine);
    for (const k of [remoteKey(), pendingKey()]) {
      try {
        localStorage.removeItem(k);
      } catch (e) {
        /* ignore */
      }
    }
    server = {};
    store.set("pantheon.gm.server", null);
    srvError = null;
    srvLast = null;
    loadJournal();
    messages($("#sh-srv-messages"));
    refresh({ sync: false });
  }

  // ---------------------------------------------------------------- Tabs
  function showTab(name) {
    if (!$("#tab-" + name)) name = "bazar";
    for (const b of $$(".tabs button")) b.setAttribute("aria-selected", String(b.dataset.tab === name));
    for (const s of $$(".tab")) s.hidden = s.id !== "tab-" + name;
    store.set("pantheon.gm.tab", name);
    unpin();
  }

  // ---------------------------------------------------------------- Tooltips
  let pinned = null;
  const tip = () => $("#tip");
  function showTip(anchor) {
    const u = unitById(anchor.dataset.id);
    if (!u) return;
    tip().replaceChildren(...tipBody(u));
    placeTip(tip(), anchor);
  }
  function hideTip() {
    if (!pinned) tip().hidden = true;
  }
  function unpin() {
    if (pinned) pinned.classList.remove("pinned");
    pinned = null;
    tip().hidden = true;
  }
  function unitIcon(u) {
    const s = el("span", { className: "bz-unit", textContent: u.emoji, tabIndex: 0 });
    s.dataset.id = u.id;
    s.setAttribute("aria-label", u.name);
    return s;
  }

  // ---------------------------------------------------------------- Bazar
  const timeLabel = (at) =>
    new Date(at).toLocaleString("fr-FR", { weekday: "short", day: "numeric", month: "numeric", hour: "2-digit", minute: "2-digit" });

  function shopBoxes(shops) {
    return shops.map((s) =>
      el(
        "div",
        { className: "tier-box" },
        el("h3", { className: "tier-title", textContent: `Niveau ${s.level}` }),
        s.units.length
          ? el(
              "ul",
              { className: "bz-list" },
              ...s.units
                .map((u) => (typeof u === "string" ? unitById(u) : unitById(u.id)))
                .filter(Boolean)
                .map((u) => {
                  const r = Engine.info.rules;
                  const left = view && view.stock ? view.stock[u.id] : null;
                  return el(
                    "li",
                    {},
                    unitIcon(u),
                    el("span", { className: "bz-name", textContent: u.name }),
                    left === null ? "" : el("span", { className: "bz-left" + (left === 0 ? " zero" : ""), textContent: `reste ${left}`, title: "Exemplaires restants dans la réserve" }),
                    el("span", { className: "bz-price", textContent: `${u.cost - r.post_cost}🙏` }),
                  );
                }),
            )
          : el("p", { className: "hint", textContent: "Réserve vide : rien à vendre." }),
      ),
    );
  }

  function renderCurrentShop() {
    const s = view.shop;
    $("#bz-current-actions").hidden = !s;
    if (!s) {
      $("#bz-current-meta").textContent = "Aucune boutique enregistrée pour l'instant.";
      $("#bz-current").replaceChildren();
      return;
    }
    const twice = (view.pages.get(s.page) || []).filter((e) => view.counts(e)).length > 1;
    $("#bz-current-meta").textContent =
      `Page ${s.page} · tirée par ${s.by}, ${timeLabel(s.at)}` + (twice ? " · ⚠ cette page a été tirée plusieurs fois" : "");
    $("#bz-current").replaceChildren(...shopBoxes(s.shops));
  }

  let pageTouched = false;
  let draft = null;

  function drawShops() {
    unpin();
    const levels = $$(".bz-levels input:checked").map((c) => Number(c.value));
    store.set("pantheon.gm.levels", levels);
    const page = Math.floor(Number($("#bz-page").value) || 0);
    const [a, b] = crypto.getRandomValues(new Uint32Array(2));
    const seed = (a % 100000) * 100000 + (b % 100000);
    if (!view.stock) {
      messages($("#bz-messages"), ["Créez d'abord la réserve commune (onglet Réserve)."]);
      return;
    }
    let r;
    try {
      r = Engine.call({ op: "shop", levels, page: page > 0 ? page : null, players: view.players, stock: view.stock, seed });
    } catch (e) {
      messages($("#bz-messages"), [e.message]);
      return;
    }
    if (!r.ok) {
      messages($("#bz-messages"), r.errors);
      $("#bz-result").hidden = true;
      draft = null;
      return;
    }
    const warnings = [...r.warnings];
    const done = (view.pages.get(page) || []).filter((e) => view.counts(e));
    if (done.length) warnings.unshift(`La page ${page} a déjà été tirée par ${done.map((e) => e.by).join(", ")} : vérifiez le numéro de page.`);
    messages($("#bz-messages"), [], warnings);
    draft = { page, levels, seed, shops: r.shops.map((s) => ({ level: s.level, units: s.units.map((u) => u.id) })), forum: r.forum };
    $("#bz-result-page").textContent = page > 0 ? String(page) : "";
    $("#bz-shops").replaceChildren(...shopBoxes(draft.shops));
    $("#bz-forum").value = r.forum;
    $("#bz-preview").innerHTML = renderTwino(r.forum);
    $("#bz-result").hidden = false;
    $("#bz-result").scrollIntoView({ behavior: "smooth", block: "start" });
  }

  async function copyAndRecord(ev) {
    if (!draft) return;
    await copyText(draft.forum, ev.currentTarget);
    record("shop", draft);
    messages($("#bz-messages"), [], [], [`Boutique de la page ${draft.page} enregistrée : c'est la boutique en cours, publiée sur le Bazar en ligne.`]);
    draft = null;
    pageTouched = false;
    $("#bz-result").hidden = true;
    renderBazar();
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  function renderBazar() {
    renderCurrentShop();
    if (!pageTouched) $("#bz-page").value = String(view.nextPage);
    $("#bz-draw").disabled = !view.stock;
    if (!view.stock && !draft) messages($("#bz-messages"), [], ["La réserve commune n'existe pas encore : créez-la dans l'onglet Réserve."]);
    else if (view.stock && $("#bz-messages").textContent.startsWith("La réserve commune n'existe pas")) messages($("#bz-messages"));
  }

  // ---------------------------------------------------------------- Reserve
  const stockInputs = new Map(); // unit id → { row, input }

  function buildStock() {
    const roster = Engine.info.roster;
    const tiers = [...new Set(roster.map((u) => u.tier))];
    $("#rs-stock").replaceChildren(
      ...tiers.map((t) =>
        el(
          "div",
          { className: "tier-box" },
          el("h3", { className: "tier-title", textContent: `Rang ${t}` }),
          el(
            "div",
            { className: "stock-list" },
            ...roster
              .filter((u) => u.tier === t)
              .map((u) => {
                const input = el("input", { type: "number", min: "0", max: String(MAX_COPIES), value: "0" });
                input.setAttribute("aria-label", `Exemplaires restants : ${u.name}`);
                const minus = el("button", { type: "button", className: "step", textContent: "−", title: `Achat : un ${u.name} quitte la réserve` });
                const plus = el("button", { type: "button", className: "step", textContent: "+", title: `Retour : un ${u.name} revient dans la réserve` });
                const row = el("div", { className: "stock-row" }, unitIcon(u), el("span", { className: "stock-name", textContent: u.name }), minus, input, plus);
                minus.addEventListener("click", () => view.stock && view.stock[u.id] > 0 && record("move", { unit: u.id, delta: -1, reason: "achat" }));
                plus.addEventListener("click", () => view.stock && record("move", { unit: u.id, delta: 1, reason: "retour" }));
                input.addEventListener("change", () => {
                  if (!view.stock) return;
                  const v = Math.floor(Number(input.value));
                  if (!Number.isFinite(v) || v < 0 || v > MAX_COPIES) {
                    input.value = String(view.stock[u.id]);
                    return;
                  }
                  if (v !== view.stock[u.id]) record("set", { unit: u.id, count: v });
                });
                input.addEventListener("keydown", (ev) => ev.key === "Enter" && input.blur());
                stockInputs.set(u.id, { row, input, minus, plus });
                return row;
              }),
          ),
        ),
      ),
    );
  }

  function renderReserve() {
    const stock = view.stock;
    for (const [id, { row, input, minus, plus }] of stockInputs) {
      const n = stock ? stock[id] : 0;
      row.classList.toggle("zero", !!stock && n === 0);
      if (document.activeElement !== input) input.value = stock ? String(n) : "";
      input.disabled = plus.disabled = !stock;
      minus.disabled = !stock || n === 0;
    }
    $("#rs-reset").textContent = stock ? "Nouvelle partie" : "Créer la réserve";
    $("#rs-apply-players").hidden = !stock;
    if (stock && document.activeElement !== $("#rs-players") && !playersTouched) $("#rs-players").value = String(view.players);
    if (!stock) {
      $("#rs-state").textContent = "La réserve n'existe pas encore : entrez le nombre de joueurs inscrits puis « Créer la réserve ».";
      $("#rs-total").textContent = "";
      return;
    }
    const tiers = {};
    let total = 0;
    for (const u of Engine.info.roster) {
      tiers[u.tier] = (tiers[u.tier] || 0) + stock[u.id];
      total += stock[u.id];
    }
    $("#rs-state").textContent = `${view.players} joueurs inscrits · ${total} exemplaires dans la réserve.`;
    $("#rs-total").textContent = `Réserve : ${total} exemplaires · ` + Object.entries(tiers).map(([t, n]) => `rang ${t} : ${n}`).join(" · ");
  }

  let playersTouched = false;
  const inputPlayers = () => Math.floor(Number($("#rs-players").value) || 0);

  function applyPlayers() {
    const n = inputPlayers();
    if (!validPlayers(n)) return messages($("#rs-messages"), ["Nombre de joueurs : de 1 à 200."]);
    if (n === view.players) return messages($("#rs-messages"), [], [`La réserve est déjà prévue pour ${n} joueurs.`]);
    const diff = Engine.info.rules.pool_per_player
      .map((_, k) => poolCopies(k + 1, n) - poolCopies(k + 1, view.players))
      .map((d, k) => `rang ${k + 1} : ${d >= 0 ? "+" : ""}${d}`)
      .join(", ");
    if (!window.confirm(`Passer de ${view.players} à ${n} joueurs ?\nExemplaires par unité : ${diff}.`)) return;
    record("players", { players: n, from: view.players });
    playersTouched = false;
    messages($("#rs-messages"), [], [], [`Réserve prévue pour ${n} joueurs (${diff} par unité).`]);
  }

  function resetReserve() {
    const n = inputPlayers();
    if (!validPlayers(n)) return messages($("#rs-messages"), ["Nombre de joueurs : de 1 à 200."]);
    const ask = view.stock
      ? `Nouvelle partie : remettre la réserve complète pour ${n} joueurs ?\nTous les MJ la verront ; le journal garde l'ancienne partie.`
      : `Créer la réserve complète pour ${n} joueurs ?`;
    if (!window.confirm(ask)) return;
    record("reset", { players: n });
    playersTouched = false;
    messages($("#rs-messages"), [], [], [`Réserve complète pour ${n} joueurs.`]);
  }

  // ---------------------------------------------------------------- Journal
  let journalShown = 100;

  function describe(e) {
    const u = e.unit ? unitById(e.unit) : null;
    const left = view.after.has(e.id) ? ` (reste ${view.after.get(e.id)})` : "";
    switch (e.type) {
      case "reset":
        return `🔄 Nouvelle partie : réserve complète pour ${e.players} joueurs`;
      case "players":
        return `👥 Joueurs inscrits : ${e.from ?? "?"} → ${e.players}`;
      case "move":
        return e.delta < 0
          ? `🛒 Achat : ${u.emoji} ${u.name}${e.delta < -1 ? ` ×${-e.delta}` : ""}${left}`
          : `↩️ Retour : ${u.emoji} ${u.name}${e.delta > 1 ? ` ×${e.delta}` : ""}${left}`;
      case "set":
        return `✏️ Correction : ${u.emoji} ${u.name} à ${e.count}`;
      case "shop":
        return `🏪 Boutique de la page ${e.page} (niveau${e.shops.length > 1 ? "x" : ""} ${e.shops.map((s) => s.level).join(", ")})`;
      case "undo": {
        const t = view.all.find((x) => x.id === e.target);
        return `↶ Annule : ${t ? describe(t) : "une action inconnue"}`;
      }
      case "player": {
        const what = [];
        if (e.taken) what.push(`achète ${unitList([e.taken])}`);
        if (e.returned.length) what.push(`rend ${unitList(e.returned)}`);
        return `🏪 Bazar n°${e.srv}, ${e.player}${e.verified ? " ✔" : ""} : ${what.length ? what.join(" · ") : "passe"}` + (e.page ? ` (page ${e.page})` : "");
      }
      default:
        return e.type;
    }
  }

  function renderJournal() {
    const list = view.all.slice().reverse();
    $("#journal").replaceChildren(
      ...(list.length
        ? list.slice(0, journalShown).map((e) => {
            const on = view.counts(e);
            const li = el(
              "li",
              { className: on ? "" : "undone" },
              el("span", { className: "j-when", textContent: timeLabel(e.at) }),
              el("span", { className: "j-by", textContent: e.by || "MJ" }),
              el("span", { className: "j-what", textContent: describe(e) }),
            );
            const b = el("button", { type: "button", className: "ghost j-undo", textContent: on ? (e.type === "undo" ? "Rétablir" : "Annuler") : "Annulé" });
            b.disabled = !on;
            if (Number.isInteger(e.srv) && e.type === "undo") {
              // A cancellation made on the Bazar server stays there.
              b.textContent = "via le Bazar";
              b.disabled = true;
            } else if (Number.isInteger(e.srv)) {
              b.title = "Annule ce message sur le serveur du Bazar, pour le joueur aussi";
              b.addEventListener("click", () => cancelOnServer(e, b));
            } else {
              b.addEventListener("click", () => record("undo", { target: e.id }));
            }
            li.append(b);
            return li;
          })
        : [el("li", { className: "hint", textContent: "Aucune action pour l'instant." })]),
    );
    $("#journal-more").hidden = list.length <= journalShown;
  }

  // ---------------------------------------------------------------- Rendering
  // `sync`: also send the change to the server soon (not when the change came from it).
  function refresh({ sync = true } = {}) {
    if (!Engine.info) return;
    replay();
    renderBazar();
    renderReserve();
    renderJournal();
    renderServer();
    if (sync) serverSoon();
  }

  // The reserve typed in the old Bazar tab of the public page becomes the first local events.
  function migrateOldTab() {
    if (allEvents().length) return;
    const old = store.get("pantheon.bazar.stock", null);
    if (!old || typeof old !== "object") return;
    const players = store.get("pantheon.bazar.players", Engine.info.rules.players);
    if (!validPlayers(players)) return;
    const full = fullStock(players);
    record("reset", { players });
    for (const u of Engine.info.roster) {
      const v = Number(old[u.id]);
      if (Number.isInteger(v) && v >= 0 && v <= MAX_COPIES && v !== full[u.id]) record("set", { unit: u.id, count: v });
    }
    try {
      localStorage.removeItem("pantheon.bazar.stock");
    } catch (e) {
      /* ignore */
    }
  }

  function wire() {
    for (const b of $$(".tabs button")) b.addEventListener("click", () => showTab(b.dataset.tab));
    // Tooltips: hover, focus, or a click that pins them.
    document.addEventListener("pointerover", (ev) => {
      const t = ev.target.closest(".bz-unit");
      if (t && ev.pointerType === "mouse" && !pinned) showTip(t);
    });
    document.addEventListener("pointerout", (ev) => ev.pointerType === "mouse" && ev.target.closest(".bz-unit") && hideTip());
    document.addEventListener("focusin", (ev) => {
      const t = ev.target.closest(".bz-unit");
      if (t && !pinned) showTip(t);
    });
    document.addEventListener("focusout", (ev) => ev.target.closest(".bz-unit") && hideTip());
    document.addEventListener("click", (ev) => {
      const t = ev.target.closest(".bz-unit");
      if (!t) {
        if (pinned && !ev.target.closest("#tip")) unpin();
        return;
      }
      if (pinned === t) return unpin();
      unpin();
      pinned = t;
      t.classList.add("pinned");
      showTip(t);
    });
    document.addEventListener("keydown", (ev) => ev.key === "Escape" && unpin());
    document.addEventListener("scroll", hideTip, { passive: true });

    $("#bz-draw").addEventListener("click", drawShops);
    $("#bz-redraw").addEventListener("click", drawShops);
    $("#bz-copy").addEventListener("click", copyAndRecord);
    $("#bz-page").addEventListener("input", () => (pageTouched = true));
    $("#bz-current-copy").addEventListener("click", (ev) => view.shop && copyText(view.shop.forum, ev.currentTarget));
    const levels = store.get("pantheon.gm.levels", null);
    if (Array.isArray(levels)) for (const c of $$(".bz-levels input")) c.checked = levels.includes(Number(c.value));

    $("#rs-players").addEventListener("input", () => (playersTouched = true));
    $("#rs-apply-players").addEventListener("click", applyPlayers);
    $("#rs-reset").addEventListener("click", resetReserve);
    $("#journal-more").addEventListener("click", () => {
      journalShown += 200;
      renderJournal();
    });

    const name = $("#sh-name");
    name.value = settings.name || "";
    name.addEventListener("input", () => {
      settings.name = name.value.slice(0, 40);
      saveSettings();
    });
    $("#sh-export").addEventListener("click", exportJournal);
    $("#sh-import").addEventListener("change", (ev) => importJournal(ev.currentTarget));
    $("#sh-import-btn").addEventListener("click", () => $("#sh-import").click());
    $("#sh-srv-save").addEventListener("click", saveServer);
    $("#sh-srv-forget").addEventListener("click", forgetServer);
    pushed = store.get(pushedKey(), {}) || {};

    setInterval(() => document.visibilityState === "visible" && serverSync(), SERVER_POLL_MS);
    // Another tab of this browser changed the journal or the settings.
    window.addEventListener("storage", (ev) => {
      if (!ev.key || !ev.key.startsWith("pantheon.gm.")) return;
      settings = store.get("pantheon.gm.settings", {}) || {};
      server = store.get("pantheon.gm.server", null) || server;
      loadJournal();
      refresh({ sync: false });
    });
    document.addEventListener("visibilitychange", () => document.visibilityState === "visible" && serverSync());
    window.addEventListener("online", serverSync);
  }

  async function start() {
    // An old invitation link (shared reserve on GitHub): its secret leaves the address bar.
    if (location.hash.startsWith("#rejoindre=")) {
      try {
        history.replaceState(null, "", location.pathname + location.search);
      } catch (e) {
        /* ignore */
      }
    }
    migrateGist();
    wire();
    loadJournal();
    const status = $("#engine-status");
    try {
      await Engine.load();
    } catch (e) {
      status.textContent = "Le moteur n'a pas pu démarrer : " + e.message;
      status.classList.add("error");
      return;
    }
    status.hidden = true;
    const info = Engine.info;
    $("#footer").textContent = `Moteur ${info.version} · données ${info.data_hash} · commit ${window.PANTHEON_GIT || "inconnu"}`;
    $("#rs-rates").textContent =
      info.rules.pool_per_player.map((r, k) => `rang ${k + 1} : ${String(r).replace(".", ",")}`).join(" · ") +
      ` exemplaire par unité, arrondi, jamais moins de ${info.rules.merge_copies}`;
    $("#rs-players").value = String(info.rules.players);
    buildStock();
    migrateOldTab();
    refresh();
    showTab(store.get("pantheon.gm.tab", "bazar"));
  }

  start();
})();
