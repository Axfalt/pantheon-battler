// Pantheon Battler: the GM tools (/gm_tool/). The Bazar draw, the common reserve and its journal,
// shared between GMs through a secret GitHub Gist.
//
// Everything a GM does is an event appended to the journal of this browser. Each browser writes
// only its own file of the Gist (journal-<device>.json), so GMs never overwrite each other; the
// reserve is recomputed by replaying the events of every journal in time order.
"use strict";

(function () {
  const { $, $$, el, Engine, store, copyText, messages, renderTwino, unitById, tipBody, placeTip } = window.PB;

  const API = "https://api.github.com";
  const MARKER = "pantheon-mj.json"; // the file that marks a Gist as a Pantheon Battler reserve
  const JOURNAL_RE = /^journal-[\w-]{1,40}\.json$/;
  const POLL_MS = 20000;
  const MAX_COPIES = 999;

  // ---------------------------------------------------------------- Settings and journals
  // settings: { name, token, gist } — the token never leaves this browser except towards GitHub.
  let settings = store.get("pantheon.gm.settings", {}) || {};
  let device = store.get("pantheon.gm.device", null);
  if (typeof device !== "string" || !/^[a-z0-9]{6,12}$/.test(device)) {
    device = Array.from(crypto.getRandomValues(new Uint8Array(6)), (b) => (b % 36).toString(36)).join("");
    store.set("pantheon.gm.device", device);
  }
  const myFile = `journal-${device}.json`;
  const connected = () => !!(settings.gist && settings.token);
  const place = () => (connected() ? settings.gist : "local");

  let mine = []; // this browser's events, append-only
  let synced = 0; // how many of them GitHub has
  let others = {}; // file name → { name, events } for the other journals of the Gist
  let nameDirty = false; // the GM name changed: this browser's journal file must be rewritten
  let lastSync = null; // Date of the last successful exchange with GitHub
  let syncError = null;
  let gistOwner = "";

  function loadJournal() {
    mine = store.get(`pantheon.gm.journal.${place()}`, []);
    if (!Array.isArray(mine)) mine = [];
    synced = connected() ? Math.min(mine.length, store.get(`pantheon.gm.synced.${place()}`, 0) || 0) : mine.length;
    others = connected() ? store.get(`pantheon.gm.others.${place()}`, {}) || {} : {};
  }
  function saveJournal() {
    store.set(`pantheon.gm.journal.${place()}`, mine);
    if (connected()) {
      store.set(`pantheon.gm.synced.${place()}`, synced);
      store.set(`pantheon.gm.others.${place()}`, others);
    }
  }
  const saveSettings = () => store.set("pantheon.gm.settings", settings);
  const gmName = () => (settings.name || "").trim() || "MJ";

  // Another tab of this browser may have written to the journal meanwhile.
  function reloadMine() {
    const stored = store.get(`pantheon.gm.journal.${place()}`, []);
    if (Array.isArray(stored) && stored.length > mine.length) mine = stored;
  }
  const needPush = () => synced < mine.length || nameDirty;

  // Records an action: replayed at once, sent to GitHub soon after.
  function record(type, fields) {
    reloadMine();
    const e = { id: `${device}.${(mine.length + 1).toString(36)}`, at: Date.now(), by: gmName(), type, ...fields };
    mine.push(e);
    saveJournal();
    refresh();
    syncSoon();
    return e;
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
      default:
        return false;
    }
  }

  let view = null; // the last replay

  function replay() {
    const seen = new Set();
    const all = [];
    for (const list of [mine, ...Object.values(others).map((j) => j.events || [])]) {
      for (const e of list) {
        if (validEvent(e) && !seen.has(e.id)) {
          seen.add(e.id);
          all.push(e);
        }
      }
    }
    all.sort((a, b) => a.at - b.at || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
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
      } else if (e.type === "shop") {
        shop = e;
        maxPage = Math.max(maxPage, e.page);
        pages.set(e.page, [...(pages.get(e.page) || []), e]);
      }
    }
    view = { all, counts, stock, players, shop, nextPage: maxPage + 1, after, pages };
    return view;
  }

  // ---------------------------------------------------------------- GitHub Gist
  class GistError extends Error {}

  async function gh(method, path, body) {
    let r;
    try {
      r = await fetch(API + path, {
        method,
        cache: "no-cache",
        headers: {
          Accept: "application/vnd.github+json",
          Authorization: `Bearer ${settings.token}`,
          ...(body ? { "Content-Type": "application/json" } : {}),
        },
        body: body ? JSON.stringify(body) : undefined,
      });
    } catch (e) {
      throw new GistError("GitHub ne répond pas (connexion coupée ?). Les actions attendent et partiront plus tard.");
    }
    if (r.ok) return r.json();
    const why = {
      401: "GitHub refuse le jeton : il a expiré ou a été supprimé. Demandez un nouveau lien d'invitation.",
      403: "GitHub refuse l'accès (jeton sans la case « gist », ou trop de requêtes : réessayez dans quelques minutes).",
      404: "Gist introuvable : l'identifiant est faux, ou le jeton n'appartient pas au compte qui possède le Gist.",
      409: "GitHub était occupé par une autre modification : nouvel essai au prochain passage.",
      422: "GitHub a refusé le contenu envoyé.",
    }[r.status];
    throw new GistError(why || `GitHub a répondu ${r.status}.`);
  }

  async function fileText(f) {
    if (!f.truncated) return f.content;
    const r = await fetch(f.raw_url, { cache: "no-cache" });
    if (!r.ok) throw new GistError(`Impossible de lire ${f.filename} sur GitHub.`);
    return r.text();
  }

  // Reads the other journals out of a Gist answer.
  async function absorb(gist) {
    if (!gist.files || !gist.files[MARKER]) throw new GistError("Ce Gist n'est pas une réserve de Pantheon Battler.");
    gistOwner = (gist.owner && gist.owner.login) || "";
    const next = {};
    for (const [name, f] of Object.entries(gist.files)) {
      if (name === myFile || !JOURNAL_RE.test(name)) continue;
      try {
        const j = JSON.parse(await fileText(f));
        next[name] = { name: String(j.name || "MJ").slice(0, 40), events: Array.isArray(j.events) ? j.events : [] };
      } catch (e) {
        next[name] = others[name] || { name: "?", events: [] }; // a broken file: keep what we had
      }
    }
    others = next;
  }

  const journalContent = () =>
    `{"name":${JSON.stringify(gmName())},"device":"${device}","events":[\n` + mine.map((e) => JSON.stringify(e)).join(",\n") + "\n]}\n";

  let busy = false;
  let again = false;

  async function sync() {
    if (!connected()) return;
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    renderBadge();
    try {
      reloadMine();
      const sending = mine.length;
      let gist;
      if (needPush()) {
        gist = await gh("PATCH", `/gists/${settings.gist}`, { files: { [myFile]: { content: journalContent() } } });
        synced = sending;
        nameDirty = false;
      } else {
        gist = await gh("GET", `/gists/${settings.gist}`);
      }
      await absorb(gist);
      lastSync = new Date();
      syncError = null;
      saveJournal();
      refresh();
    } catch (e) {
      syncError = e instanceof GistError ? e.message : `Erreur de synchronisation : ${e.message}`;
    } finally {
      busy = false;
      renderBadge();
    }
    if (again || needPush()) {
      again = false;
      setTimeout(sync, syncError ? 5000 : 300);
    }
  }

  let soon = null;
  function syncSoon() {
    clearTimeout(soon);
    soon = setTimeout(sync, 400);
  }

  // ---------------------------------------------------------------- Header badge
  function renderBadge() {
    const badge = $("#sync-badge");
    const pending = mine.length - synced;
    let state;
    if (pending === 0 && nameDirty && !syncError) {
      badge.dataset.state = "busy";
      badge.textContent = "Synchronisation…";
      return;
    }
    let text;
    if (!connected()) {
      state = "local";
      text = "Ce navigateur seulement · onglet Partage pour partager";
    } else if (syncError) {
      state = "error";
      text = `⚠ ${syncError}` + (pending > 0 ? ` (${pending} action${pending > 1 ? "s" : ""} en attente)` : "");
    } else if (busy || pending > 0) {
      state = "busy";
      text = "Synchronisation…";
    } else {
      state = "ok";
      const gms = new Set(Object.values(others).map((j) => j.name).concat(gmName())).size;
      text = `☁ Réserve partagée · ${gms} MJ` + (lastSync ? ` · à jour ${lastSync.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}` : "");
    }
    badge.dataset.state = state;
    badge.textContent = text;
    const s = $("#sh-sync");
    if (s) s.textContent = text;
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
    messages($("#bz-messages"), [], [], [`Boutique de la page ${draft.page} enregistrée : c'est la boutique en cours pour tous les MJ.`]);
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
            b.addEventListener("click", () => record("undo", { target: e.id }));
            li.append(b);
            return li;
          })
        : [el("li", { className: "hint", textContent: "Aucune action pour l'instant." })]),
    );
    $("#journal-more").hidden = list.length <= journalShown;
  }

  // ---------------------------------------------------------------- Sharing
  const b64url = (s) => {
    const bytes = new TextEncoder().encode(s);
    let bin = "";
    bytes.forEach((b) => (bin += String.fromCharCode(b)));
    return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  };
  const unb64url = (s) => {
    const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
    const bin = atob(b64 + "===".slice((b64.length + 3) % 4));
    return new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
  };
  const inviteLink = () => `${location.origin}${location.pathname}#rejoindre=${b64url(JSON.stringify({ g: settings.gist, t: settings.token }))}`;

  function parseInvite(text) {
    const m = String(text).match(/#rejoindre=([\w-]+)/) || String(text).match(/^([\w-]{20,})$/);
    if (!m) return null;
    try {
      const j = JSON.parse(unb64url(m[1]));
      return typeof j.g === "string" && /^[0-9a-f]{20,40}$/.test(j.g) && typeof j.t === "string" && j.t.length > 10 ? j : null;
    } catch (e) {
      return null;
    }
  }

  function renderShare() {
    $("#sh-connected").hidden = !connected();
    $("#sh-setup").hidden = connected();
    if (!connected()) return;
    const a = $("#sh-gist-link");
    a.href = `https://gist.github.com/${gistOwner ? gistOwner + "/" : ""}${settings.gist}`;
    a.textContent = settings.gist.slice(0, 8) + "…" + (gistOwner ? ` (compte ${gistOwner})` : "");
    $("#sh-invite").value = inviteLink();
    const gms = [{ name: gmName() + " (vous)", at: mine.length ? mine[mine.length - 1].at : 0 }].concat(
      Object.values(others).map((j) => ({ name: j.name, at: j.events.length ? j.events[j.events.length - 1].at : 0 })),
    );
    $("#sh-gms").textContent =
      "Journaux : " + gms.map((g) => `${g.name}${g.at ? ` (dernière action ${timeLabel(g.at)})` : " (aucune action)"}`).join(" · ");
  }

  async function connect(gist, token, { create = false } = {}) {
    const before = { ...settings };
    const localEvents = mine.slice();
    settings = { ...settings, gist, token };
    try {
      if (create) {
        const created = await gh("POST", "/gists", {
          description: "Pantheon Battler · réserve commune du Bazar (MJ)",
          public: false,
          files: {
            [MARKER]: { content: JSON.stringify({ app: "pantheon-battler", kind: "bazar-reserve", version: 1, created: new Date().toISOString() }, null, 2) + "\n" },
          },
        });
        settings.gist = created.id;
      }
      saveSettings();
      loadJournal();
      // The reserve kept in this browser becomes the shared one when the GM creates it.
      if (create && mine.length === 0 && localEvents.length) {
        mine = localEvents;
        synced = 0;
      }
      const gist = await gh(synced < mine.length || create ? "PATCH" : "GET", `/gists/${settings.gist}`,
        synced < mine.length || create ? { files: { [myFile]: { content: journalContent() } } } : undefined);
      synced = mine.length;
      await absorb(gist);
      lastSync = new Date();
      syncError = null;
      saveJournal();
      return true;
    } catch (e) {
      settings = before;
      saveSettings();
      loadJournal();
      throw e;
    } finally {
      refresh();
    }
  }

  async function join() {
    const inv = parseInvite($("#sh-join-link").value.trim());
    if (!inv) return messages($("#sh-join-messages"), ["Lien d'invitation illisible : copiez-le en entier."]);
    if (!(settings.name || "").trim()) return messages($("#sh-join-messages"), ["Entrez d'abord votre nom de MJ, en haut de cet onglet."]);
    const b = $("#sh-join");
    b.disabled = true;
    try {
      await connect(inv.g, inv.t);
      $("#sh-join-link").value = "";
      messages($("#sh-join-messages"));
      messages($("#sh-messages"), [], [], ["Vous avez rejoint la réserve partagée."]);
    } catch (e) {
      messages($("#sh-join-messages"), [e.message]);
    } finally {
      b.disabled = false;
    }
  }

  async function create() {
    const token = $("#sh-token").value.trim();
    if (!(settings.name || "").trim()) return messages($("#sh-create-messages"), ["Entrez d'abord votre nom de MJ, en haut de cet onglet."]);
    if (token.length < 20) return messages($("#sh-create-messages"), ["Collez le jeton GitHub (il commence par ghp_)."]);
    const b = $("#sh-create");
    b.disabled = true;
    try {
      await connect(null, token, { create: true });
      $("#sh-token").value = "";
      messages($("#sh-create-messages"));
      messages($("#sh-messages"), [], [], ["Réserve partagée créée. Envoyez le lien d'invitation aux autres MJ, en message privé."]);
    } catch (e) {
      messages($("#sh-create-messages"), [e.message]);
    } finally {
      b.disabled = false;
    }
  }

  function logout() {
    const pending = mine.length - synced;
    const ask =
      (pending > 0 ? `${pending} action(s) ne sont pas encore parties sur GitHub et seront perdues.\n` : "") +
      "Déconnecter ce navigateur de la réserve partagée ? Le jeton y sera effacé ; le lien d'invitation permet de revenir.";
    if (!window.confirm(ask)) return;
    store.set(`pantheon.gm.journal.${settings.gist}`, []);
    store.set(`pantheon.gm.synced.${settings.gist}`, 0);
    store.set(`pantheon.gm.others.${settings.gist}`, {});
    settings = { name: settings.name };
    saveSettings();
    syncError = null;
    lastSync = null;
    loadJournal();
    refresh();
  }

  // ---------------------------------------------------------------- Rendering
  function refresh() {
    if (!Engine.info) return;
    replay();
    renderBazar();
    renderReserve();
    renderJournal();
    renderShare();
    renderBadge();
  }

  // The reserve typed in the old Bazar tab of the public page becomes the first local events.
  function migrateOldTab() {
    if (connected() || mine.length) return;
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
      renderShare();
    });
    // A new name reaches the other GMs at once.
    name.addEventListener("change", () => {
      if (!connected()) return;
      nameDirty = true;
      syncSoon();
    });
    $("#sh-join").addEventListener("click", join);
    $("#sh-create").addEventListener("click", create);
    $("#sh-logout").addEventListener("click", logout);
    $("#sh-sync-now").addEventListener("click", sync);
    $("#sh-invite-copy").addEventListener("click", (ev) => copyText($("#sh-invite").value, ev.currentTarget));

    setInterval(() => document.visibilityState === "visible" && sync(), POLL_MS);
    // Another tab of this browser changed the journal or the settings.
    window.addEventListener("storage", (ev) => {
      if (!ev.key || !ev.key.startsWith("pantheon.gm.")) return;
      settings = store.get("pantheon.gm.settings", {}) || {};
      loadJournal();
      refresh();
    });
    document.addEventListener("visibilitychange", () => document.visibilityState === "visible" && sync());
    window.addEventListener("online", sync);
  }

  async function start() {
    // An invitation link: its secret leaves the address bar at once.
    const inv = location.hash.startsWith("#rejoindre=") ? location.hash : null;
    if (inv) {
      try {
        history.replaceState(null, "", location.pathname + location.search);
      } catch (e) {
        /* ignore */
      }
    }
    wire();
    loadJournal();
    renderBadge();
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
    if (inv) {
      showTab("partage");
      const parsed = parseInvite(inv);
      $("#sh-join-link").value = inv;
      if (!parsed) {
        messages($("#sh-join-messages"), ["Lien d'invitation illisible : copiez-le en entier."]);
      } else if (connected() && parsed.g === settings.gist) {
        messages($("#sh-messages"), [], [], ["Ce navigateur est déjà connecté à cette réserve."]);
      } else if (connected()) {
        messages($("#sh-messages"), [], ["Ce lien mène à une autre réserve : déconnectez d'abord ce navigateur, puis « Rejoindre »."]);
      } else {
        messages($("#sh-join-messages"), [], [], ["Lien d'invitation reçu : entrez votre nom de MJ puis « Rejoindre »."]);
        if (!(settings.name || "").trim()) $("#sh-name").focus();
      }
    }
    sync();
  }

  start();
})();
