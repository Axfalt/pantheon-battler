// Pantheon Battler: the Bazar tab of the players' page. The player keeps their state here
// (field, reserve, favors, shop level), picks the orders of their next Bazar post against the
// live shop, and confirms it on the Bazar server: the unit is then gone for the others, and the
// post is copied, ready to paste on the forum. A confirmed post can be cancelled.
//
// The orders and the post text come from the engine (op bazar_apply); the server (config.js,
// window.PANTHEON_API) orders the purchases and keeps each player's god in their account
// (server/handler.mjs): made at the first confirmed post (the browser keeps its session), or
// from the player's MyHordes key (optional), and opened on another device with a code.
// The Compte tab shows that account: its settings, the god (the same editor as the Bazar tab,
// moved there while the tab is open) with its challenge composition for the forum, and the
// history of its posts.
"use strict";

(function () {
  const { $, $$, el, Engine, store, copyText, messages, renderTwino, unitById, baseStats, tipBody, placeTip } = window.PB;
  const API = (window.PANTHEON_API || "").replace(/\/+$/, "");
  const POLL_MS = 15000;
  const KEY = "pantheon.bazar.state";
  const SESSION = `pantheon.bazar.session.${API}`; // this browser's session on that server

  let token = API ? store.get(SESSION, null) : null;
  let account = null; // { id, name, verified } once logged in
  let version = null; // the account's version our god was read at
  let myPosts = []; // the account's last posts: { seq, at, page, forum, cancelled, restored }
  let rules = null;
  let me = null; // PlayerState: { name, favors, level, invested, board: [entry], reserve: [entry] }
  let buy = null; // { level, slot, unit }: the purchase of this post
  let selling = new Set(); // entries of `me` sold in this post
  let invest = 0;
  let viewLevel = null; // the shop shown (the player's own unless they look at another)
  let remote = null; // the server's answer to GET /bazar
  let remoteError = null;
  let result = null; // the engine's answer for the post being prepared
  let selected = null; // the entry whose editor is open

  // vp/essence/streak/wins: the arena numbers, only for the challenge composition (Compte tab).
  const emptyState = () => ({ name: "", favors: 0, level: 1, invested: 0, board: [], reserve: [], vp: 0, essence: null, streak: 0, wins: 0 });
  const ARENA = ["vp", "essence", "streak", "wins"];
  const arena = (s) => Object.fromEntries(ARENA.map((k) => [k, s[k] ?? null]));
  const clone = (x) => JSON.parse(JSON.stringify(x));
  const fold = (s) => String(s || "").trim().toLocaleLowerCase("fr");
  const time = (at) => new Date(at).toLocaleString("fr-FR", { weekday: "short", hour: "2-digit", minute: "2-digit" });
  const unitText = (id) => {
    const u = unitById(id);
    return u ? `${u.emoji} ${u.name}` : id;
  };
  // A god's numbers and units, to tell whether it changed (the server fills in missing fields).
  const canon = (s) =>
    JSON.stringify([s.favors, s.level, s.invested, ...ARENA.map((k) => s[k] ?? null), ...[s.board, s.reserve].map((l) => l.map((e) => [e.id, !!e.star, e.atk, e.hp, e.def, e.deaths || 0]))]);
  const firstLine = (text) => String(text || "").split("\n")[0].replace(/\[\/?b\]/g, "");

  // The god is kept in this browser, and in the account when logged in (saved shortly after
  // each change).
  function save() {
    store.set(KEY, me);
    if (account) schedulePush();
  }

  // ---------------------------------------------------------------- Server
  async function api(method, path, body, { auth = true } = {}) {
    let r;
    try {
      r = await fetch(API + path, {
        method,
        cache: "no-store",
        headers: { ...(body ? { "Content-Type": "application/json" } : {}), ...(auth && token ? { Authorization: `Bearer ${token}` } : {}) },
        body: body ? JSON.stringify(body) : undefined,
      });
    } catch (e) {
      throw new Error("Le serveur du Bazar ne répond pas : vérifiez votre connexion.");
    }
    let j = null;
    try {
      j = await r.json();
    } catch (e) {
      /* not JSON */
    }
    if (!r.ok) {
      const err = new Error((j && j.error) || `Le serveur du Bazar a répondu ${r.status}.`);
      err.status = r.status;
      err.stale = !!(j && j.stale); // the god changed elsewhere
      err.session = !(j && j.session === false); // false: this browser's session is gone
      throw err;
    }
    return j;
  }

  // ---------------------------------------------------------------- Account
  const accountMessages = (...args) => messages($("#bzp-account-messages"), ...args);

  function setToken(t) {
    token = t;
    store.set(SESSION, t);
  }

  // The server's account (GET /me, or the answer of a write): it replaces the god shown.
  function applyView(v) {
    account = v.account;
    version = v.version;
    if (v.posts) myPosts = v.posts; // a plain save doesn't send them back
    me = { ...emptyState(), ...v.state, name: v.account.name };
    store.set(KEY, me);
    selling = new Set();
    selected = null;
    renderAccount();
  }

  function forget(message) {
    setToken(null);
    account = null;
    version = null;
    myPosts = [];
    renderAccount();
    if (message) accountMessages([message]);
  }

  async function refreshMe() {
    if (!API || !token) return;
    try {
      applyView(await api("GET", "/me"));
    } catch (e) {
      if (e.status === 401) forget(e.message);
      else accountMessages([e.message]);
    }
    update();
  }

  // Changes to the god reach the account a moment later; a confirmation waits for them.
  let pushTimer = null;
  let pushing = null;
  let dirty = false;
  function schedulePush() {
    dirty = true;
    clearTimeout(pushTimer);
    pushTimer = setTimeout(flush, 1200);
  }
  async function flush() {
    clearTimeout(pushTimer);
    while (pushing) await pushing;
    if (!dirty || !account) return;
    dirty = false;
    pushing = (async () => {
      try {
        const v = await api("PUT", "/me", { version, state: me });
        version = v.version;
        if (v.posts) myPosts = v.posts;
      } catch (e) {
        if (e.stale) {
          await refreshMe();
          messages($("#bzp-state-messages"), [], [e.message]);
        } else if (e.status === 401) forget(e.message);
        else {
          dirty = true;
          messages($("#bzp-state-messages"), [`Votre dieu n'a pas pu être enregistré : ${e.message}`]);
        }
      }
    })();
    await pushing;
    pushing = null;
    if (rules) renderGod(); // "enregistré"
  }

  // A god without MyHordes is made at the first confirmed post, with the name typed.
  async function ensureAccount() {
    if (account) return;
    const name = (me.name || "").trim();
    const v = await api("POST", "/accounts", { name, state: me }, { auth: false });
    setToken(v.token);
    applyView(v);
  }

  async function loginMyHordes(input, button) {
    const key = input.value.trim();
    if (!key) return accountMessages(["Collez d'abord votre clé MyHordes."]);
    button.disabled = true;
    try {
      // Logged in without MyHordes: this god becomes the MyHordes player's.
      const v = await api("POST", "/login/myhordes", { key, state: account ? undefined : me });
      setToken(v.token);
      applyView(v);
      accountMessages([], [], [`Connecté en tant que ${v.account.name}, nom vérifié par MyHordes.`]);
    } catch (e) {
      accountMessages([e.message]);
      button.disabled = false;
    }
    update();
  }

  async function useCode(input) {
    const code = input.value.trim();
    if (!code) return;
    setToken(code);
    try {
      applyView(await api("GET", "/me"));
      accountMessages([], [], [`Dieu de ${account.name} retrouvé.`]);
    } catch (e) {
      forget(e.status === 401 ? "Code inconnu ou expiré." : e.message);
    }
    update();
  }

  async function newCode(button) {
    button.disabled = true;
    try {
      const { token: code } = await api("POST", "/sessions", {});
      const field = el("input", { type: "text", readOnly: true, value: code, className: "mono" });
      const copy = el("button", { type: "button", textContent: "Copier" });
      copy.addEventListener("click", (ev) => copyText(code, ev.currentTarget));
      $("#bzp-code-box").replaceChildren(
        el("div", { className: "bz-fill" }, el("label", { className: "field" }, "Code pour un autre appareil", field), copy),
        el("p", { className: "hint", textContent: "Sur l'autre appareil : onglet Bazar → « J'ai un code ». Ce code ouvre votre dieu : ne le donnez à personne." }),
      );
    } catch (e) {
      accountMessages([e.message]);
    } finally {
      button.disabled = false;
    }
  }

  async function logout() {
    const ask = account.verified
      ? "Se déconnecter sur ce navigateur ? Votre dieu reste enregistré : votre clé MyHordes le rouvre."
      : "Se déconnecter sur ce navigateur ? Sans code d'un autre appareil (ou clé MyHordes), vous ne pourrez plus rouvrir ce dieu.";
    if (!window.confirm(ask)) return;
    await flush();
    try {
      await api("DELETE", "/sessions");
    } catch (e) {
      /* the session ends here anyway */
    }
    forget();
    me = emptyState();
    store.set(KEY, me);
    accountMessages([], [], ["Déconnecté."]);
    update();
  }

  async function rename(input) {
    const name = input.value.trim();
    if (!account || account.verified || !name || name === account.name) return;
    await flush();
    try {
      applyView(await api("PUT", "/me", { version, name }));
      accountMessages([], [], [`Vous êtes maintenant ${account.name} au Bazar.`]);
    } catch (e) {
      input.value = account.name;
      accountMessages([e.message]);
      if (e.stale) await refreshMe();
    }
    update();
  }

  async function dropOthers(button) {
    const ask = "Déconnecter tous vos autres appareils ? Les codes déjà donnés ne marcheront plus ; ce navigateur reste connecté.";
    if (!window.confirm(ask)) return;
    button.disabled = true;
    await flush();
    try {
      applyView(await api("DELETE", "/sessions/others"));
      accountMessages([], [], ["Vos autres appareils sont déconnectés."]);
    } catch (e) {
      accountMessages([e.message]);
      if (e.stale) await refreshMe();
    } finally {
      button.disabled = false;
    }
    update();
  }

  function myHordesForm(label) {
    const input = el("input", { type: "password", autocomplete: "off", spellcheck: false });
    const go = el("button", { type: "button", textContent: label });
    go.addEventListener("click", () => loginMyHordes(input, go));
    input.addEventListener("keydown", (ev) => ev.key === "Enter" && loginMyHordes(input, go));
    return el(
      "div",
      { className: "bzp-mh" },
      el("div", { className: "bz-fill" }, el("label", { className: "field" }, "Clé MyHordes (facultatif)", input), go),
      el("p", {
        className: "hint",
        textContent: "La clé personnelle de votre compte MyHordes. Elle sert une seule fois, à vérifier votre nom auprès de MyHordes : elle n'est pas gardée.",
      }),
    );
  }

  function renderAccount() {
    const box = $("#bzp-account");
    box.hidden = !API;
    if (!API) return;
    const mhOn = !!(remote && remote.myhordes);
    const parts = [];
    if (account) {
      parts.push(
        el(
          "p",
          { className: "bzp-account-line" },
          "Connecté : ",
          el("strong", { textContent: account.name }),
          account.verified ? el("span", { className: "bzp-badge", textContent: "✔ MyHordes" }) : el("span", { className: "hint", textContent: " · retrouvé par ce navigateur" }),
        ),
      );
      if (account.created)
        parts.push(
          el("p", {
            className: "hint acc-dates",
            textContent: `Créé le ${day(account.created)} · gardé jusqu'au ${day(account.expires)} (60 jours après la dernière modification : chaque message ou changement repousse cette date).`,
          }),
        );
      if (!account.verified) {
        const input = el("input", { type: "text", maxLength: 40, value: account.name, spellcheck: false, autocomplete: "nickname" });
        const go = el("button", { type: "button", textContent: "Changer de nom" });
        go.addEventListener("click", () => rename(input));
        parts.push(el("div", { className: "bz-fill" }, el("label", { className: "field" }, "Nom sur le forum", input), go));
      }
      const code = el("button", { type: "button", className: "ghost", textContent: "Code pour un autre appareil" });
      code.addEventListener("click", () => newCode(code));
      const others = el("button", { type: "button", className: "ghost", textContent: "Déconnecter mes autres appareils" });
      others.addEventListener("click", () => dropOthers(others));
      const out = el("button", { type: "button", className: "ghost", textContent: "Se déconnecter" });
      out.addEventListener("click", logout);
      parts.push(el("div", { className: "buttons" }, code, others, out), el("div", { id: "bzp-code-box" }));
      if (!account.verified && mhOn) {
        parts.push(el("details", { className: "bzp-more" }, el("summary", { textContent: "Lier mon compte MyHordes" }), myHordesForm("Lier")));
      }
    } else {
      parts.push(
        el("p", {
          className: "hint",
          textContent:
            "Votre dieu est enregistré sur le Bazar en ligne dès votre premier message confirmé, et ce navigateur le retrouve." +
            (mhOn ? " Connectez-vous avec MyHordes (facultatif) pour prouver votre nom et le retrouver partout." : ""),
        }),
      );
      if (mhOn) parts.push(myHordesForm("Se connecter avec MyHordes"));
      const input = el("input", { type: "text", autocomplete: "off", spellcheck: false, className: "mono" });
      const go = el("button", { type: "button", textContent: "Utiliser ce code" });
      go.addEventListener("click", () => useCode(input));
      parts.push(
        el("details", { className: "bzp-more" }, el("summary", { textContent: "J'ai un code d'un autre appareil" }), el("div", { className: "bz-fill" }, el("label", { className: "field" }, "Code", input), go)),
      );
    }
    box.replaceChildren(...parts);
    // In the Bazar tab: who is playing, and the way to the Compte tab.
    const mini = $("#bzp-account-mini");
    mini.hidden = !API;
    const manage = el("button", { type: "button", className: "ghost", textContent: account ? "Mon compte" : "Se connecter" });
    manage.addEventListener("click", () => $("#tabbtn-compte").click());
    mini.replaceChildren(
      ...(account
        ? ["Connecté : ", el("strong", { textContent: account.name }), account.verified ? el("span", { className: "bzp-badge", textContent: "✔ MyHordes" }) : ""]
        : ["Pas encore de compte : il se crée à votre premier message confirmé."]),
      manage,
    );
    const name = $("#bzp-name");
    name.readOnly = !!account;
    name.title = account ? (account.verified ? "Le nom de votre compte MyHordes" : "Changez de nom dans l'onglet Compte") : "";
  }

  // ---------------------------------------------------------------- Compte tab
  const day = (at) => new Date(at).toLocaleDateString("fr-FR", { day: "numeric", month: "long", year: "numeric" });

  // The god's editor lives in the Bazar tab, and moves to the Compte tab while that one is open.
  function placeGod(name) {
    const home = $(name === "compte" ? "#acc-god-home" : "#bzp-god-home");
    const god = $("#bzp-god");
    if (god.parentElement === home) return;
    unpin();
    home.append(god);
    renderEditor(); // no "Vendre" outside the Bazar tab
  }
  const inCompte = () => !!$("#bzp-god").closest("#tab-compte");

  // The challenge composition (as in the Combat tab): the field, with the arena numbers.
  let compoText = "";
  function renderCompo() {
    for (const k of ARENA) {
      const input = $(`#acc-${k}`);
      if (document.activeElement !== input) input.value = String(me[k] ?? (k === "essence" ? rules.max_essence : 0));
    }
    const compo = { name: me.name, ...arena(me), essence: me.essence ?? rules.max_essence, units: me.board };
    let r;
    try {
      r = Engine.call({ op: "format", compo });
    } catch (e) {
      r = { errors: [e.message] };
    }
    compoText = r.text || "";
    const warnings = (r.warnings || []).filter((w) => !/absente|absents/.test(w));
    messages($("#acc-compo-messages"), me.board.length ? r.errors || [] : ["Aucune unité sur le terrain : la réserve ne combat pas."], warnings);
    $("#acc-copy").disabled = !compoText || !me.board.length;
    $("#acc-compo-preview").innerHTML = compoText ? renderTwino(compoText) : "";
  }

  // The summary line: as saved in the account (or kept by this browser before the first post).
  function renderGod() {
    const t = rules.upgrade_threshold[me.level - 1];
    $("#acc-god-meta").textContent =
      `${me.name || "Sans nom"} · ${me.favors}🙏 · niveau ${me.level}` +
      (t ? ` (${me.invested}/${t}🙏 investis vers le niveau ${me.level + 1})` : "") +
      (account ? (dirty ? " · enregistrement…" : " · enregistré sur le Bazar en ligne") : " · gardé par ce navigateur seulement (pas encore de compte)");
    renderCompo();
  }

  // The account's posts, newest first; each opens on its text (the god just after it).
  const openHistory = new Set();
  function renderHistory() {
    const box = $("#acc-history");
    if (!account) {
      box.replaceChildren(el("li", { className: "hint", textContent: "Votre historique apparaît ici une fois connecté." }));
      return;
    }
    const list = myPosts.slice().sort((a, b) => b.seq - a.seq);
    if (!list.length) {
      box.replaceChildren(el("li", { className: "hint", textContent: "Aucun message confirmé pour l'instant." }));
      return;
    }
    box.replaceChildren(
      ...list.map((p) => {
        const c = p.cancelled;
        const status = c
          ? `annulé${c.gm ? ` par ${c.by} (MJ)` : ""}` + (p.restored === false ? " · dieu non remis comme avant" : "")
          : "";
        const copy = el("button", { type: "button", className: "ghost", textContent: "Copier le message" });
        copy.addEventListener("click", (ev) => copyText(p.forum, ev.currentTarget));
        const preview = el("div", { className: "preview" });
        preview.innerHTML = renderTwino(p.forum);
        const details = el(
          "details",
          {},
          el(
            "summary",
            {},
            el("span", { className: "j-when", textContent: time(p.at) }),
            el("span", { className: "j-by", textContent: `n°${p.seq}` + (p.page ? ` · page ${p.page}` : "") }),
            el("span", { className: "j-what", textContent: firstLine(p.forum) }),
            status ? el("span", { className: "acc-status", textContent: status }) : "",
          ),
          preview,
          el("div", { className: "buttons" }, copy),
        );
        details.open = openHistory.has(p.seq);
        details.addEventListener("toggle", () => (details.open ? openHistory.add(p.seq) : openHistory.delete(p.seq)));
        return el("li", { className: c ? "undone" : "" }, details);
      }),
    );
  }

  let loading = false;
  // quiet: a message already explains what happened (a refused confirmation).
  async function load({ quiet = false } = {}) {
    if (!API || loading) return;
    loading = true;
    try {
      const before = remote && remote.shop ? remote.shop.id : null;
      const first = !remote;
      // After the first load, only the events newer than the last one held (cheap polls).
      const answer = await api("GET", "/bazar" + (remote ? `?since=${remote.seq}` : ""), null, { auth: false });
      let events = answer.events;
      if (remote && answer.since === remote.seq) {
        // Newer events on top of those held; a cancel marks the post it cancels.
        const byseq = new Map(remote.events.map((e) => [e.seq, { ...e }]));
        for (const e of answer.events) byseq.set(e.seq, e);
        for (const e of answer.events)
          if (e.type === "cancel" && byseq.has(e.target)) byseq.get(e.target).cancelled = { seq: e.seq, at: e.at, by: e.by };
        events = [...byseq.values()].sort((a, b) => b.seq - a.seq).slice(0, 20);
      }
      remote = { ...answer, events };
      remoteError = null;
      if (first) renderAccount(); // now we know whether MyHordes login is open
      // A post of mine made on another device, or cancelled by a GM: my god changed.
      const news = (e) =>
        account &&
        e.account === account.id &&
        (e.type === "post" ? !myPosts.some((p) => p.seq === e.seq) : !myPosts.some((p) => p.seq === e.target && p.cancelled));
      if (answer.events.some(news)) await refreshMe();
      if (buy && (!remote.shop || remote.shop.id !== before)) {
        buy = null;
        if (!quiet) messages($("#bzp-messages"), [], ["La boutique a changé : choisissez à nouveau votre achat."]);
      } else if (buy && !offerFree(buy.level, buy.slot)) {
        const by = slotBuyer(buy.level, buy.slot);
        buy = null;
        if (!quiet) messages($("#bzp-messages"), [], [`Votre achat n'est plus disponible${by ? ` : ${by} l'a pris` : ""}.`]);
      }
    } catch (e) {
      remoteError = e.message;
    } finally {
      loading = false;
      render();
    }
  }

  const slotBuyer = (level, slot) => {
    const s = remote && remote.slots.find((x) => x.level === level && x.slot === slot);
    return s ? s.player : null;
  };
  const left = (id) => (remote && remote.avail ? remote.avail[id] ?? 0 : Infinity);
  const offerFree = (level, slot) => {
    const shop = remote && remote.shop && remote.shop.shops.find((s) => s.level === level);
    return !!shop && !slotBuyer(level, slot) && left(shop.units[slot]) > 0;
  };
  // The units still on offer per level, for the engine's check.
  const openShops = () =>
    remote && remote.shop ? remote.shop.shops.map((s) => ({ level: s.level, units: s.units.filter((_, k) => offerFree(s.level, k)) })) : null;

  // ---------------------------------------------------------------- Orders and the post
  function levelAfter() {
    let level = me.level;
    let inv = me.invested + Math.max(0, invest);
    const t = rules.upgrade_threshold;
    while (level < rules.bazar_odds.length && inv >= t[level - 1]) {
      inv -= t[level - 1];
      level++;
    }
    return level;
  }

  function orders() {
    const sells = [];
    me.board.forEach((e, index) => selling.has(e) && sells.push({ place: "board", index }));
    me.reserve.forEach((e, index) => selling.has(e) && sells.push({ place: "reserve", index }));
    return { buy: buy ? buy.unit : null, sells, invest: Math.max(0, invest) };
  }

  function compute() {
    if (buy && buy.level !== levelAfter()) buy = null; // the investment changed the shop
    try {
      result = Engine.call({ op: "bazar_apply", state: me, orders: orders(), shops: buy ? openShops() : null });
    } catch (e) {
      result = { ok: false, errors: [e.message], notes: [] };
    }
  }

  // ---------------------------------------------------------------- Rendering
  const tip = () => $("#bzp-tip");
  let pinned = null;
  function showTip(anchor) {
    const u = unitById(anchor.dataset.id);
    if (!u) return;
    const entry = anchor.dataset.list ? me[anchor.dataset.list][Number(anchor.dataset.index)] : null;
    tip().replaceChildren(...tipBody(u, { star: !!(entry && entry.star), entry }));
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

  function renderShop() {
    const box = $("#bzp-shop");
    const meta = $("#bzp-shop-meta");
    const shop = remote && remote.shop;
    $("#bzp-offline").hidden = !!API;
    if (!API) {
      $("#bzp-offline").textContent =
        "Le Bazar en ligne n'est pas encore ouvert : vous pouvez préparer votre message ici, mais pas le confirmer.";
    }
    $("#bzp-page").textContent = shop ? `· page ${shop.page}` : "";
    if (!API) meta.textContent = "";
    else if (remoteError) meta.textContent = `⚠ ${remoteError}`;
    else if (!remote) meta.textContent = "Chargement de la boutique…";
    else if (!shop) meta.textContent = "Aucune boutique n'est ouverte pour l'instant : le MJ la poste en tête de chaque page.";
    else meta.textContent = `Postée par ${shop.by}, ${time(shop.at)} · mise à jour ${new Date(remote.at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`;
    meta.classList.toggle("error", !!remoteError);

    const myLevel = levelAfter();
    const levels = shop ? shop.shops.map((s) => s.level) : [];
    if (!levels.includes(viewLevel)) viewLevel = levels.includes(myLevel) ? myLevel : levels[0] ?? null;
    $("#bzp-levels").replaceChildren(
      ...levels.map((l) => {
        const b = el("button", { type: "button", className: "bzp-level" + (l === myLevel ? " mine" : ""), textContent: `Niveau ${l}` + (l === myLevel ? " · le vôtre" : "") });
        b.setAttribute("aria-pressed", String(l === viewLevel));
        b.addEventListener("click", () => {
          viewLevel = l;
          renderShop();
        });
        return b;
      }),
    );
    const s = shop && shop.shops.find((x) => x.level === viewLevel);
    if (!s) {
      box.replaceChildren();
      return;
    }
    const own = s.level === myLevel;
    $("#bzp-shop-hint").textContent = own
      ? "Touchez une unité pour l'acheter dans ce message (touchez-la encore pour renoncer)."
      : `Ce n'est pas votre boutique : vous achetez au niveau ${myLevel}.`;
    box.replaceChildren(
      ...s.units.map((id, slot) => {
        const u = unitById(id);
        const by = slotBuyer(s.level, slot);
        const out = !by && left(id) <= 0;
        const chosen = buy && buy.level === s.level && buy.slot === slot;
        const b = el(
          "button",
          { type: "button", className: "bzp-offer" + (by || out ? " gone" : "") + (own ? "" : " other") },
          el("span", { className: "bzp-offer-emoji", textContent: u ? u.emoji : "?" }),
          el("span", { className: "bzp-offer-name", textContent: u ? u.name : id }),
          el("span", { className: "bzp-offer-stats", textContent: u ? `${u.atk}/${u.hp}/${u.def}` : "" }),
          el("span", { className: "bzp-offer-price", textContent: by ? `par ${by}` : out ? "épuisée" : u ? `${u.cost}🙏` : "" }),
        );
        b.dataset.id = id;
        b.setAttribute("aria-pressed", String(!!chosen));
        b.disabled = !own || !!by || out;
        b.title = u ? `${u.name} · rang ${u.tier} · message complet ${u.cost}🙏 (dont ${rules.post_cost}🙏 de message)` : id;
        b.addEventListener("click", () => {
          buy = chosen ? null : { level: s.level, slot, unit: id };
          update();
        });
        return b;
      }),
    );
  }

  function slotsOf(list) {
    const size = list === "board" ? rules.board_size : rules.reserve_size;
    const units = me[list];
    return Array.from({ length: size }, (_, i) => {
      const e = units[i];
      const slot = el("div", { className: "slot bzp-slot" + (e ? " full" : "") });
      slot.dataset.list = list;
      slot.dataset.index = String(i);
      slot.append(el("span", { className: "slot-num", textContent: String(i + 1) }));
      if (!e) {
        slot.append(el("span", { className: "slot-empty", textContent: "—" }));
        return slot;
      }
      const u = unitById(e.id);
      const handle = el(
        "div",
        { className: "bzp-handle" + (selling.has(e) ? " selling" : "") + (selected === e ? " selected" : ""), tabIndex: 0 },
        el("span", { className: "slot-emoji", textContent: (e.star ? "⭐" : "") + (u ? u.emoji : "?") }),
        el("span", { className: "slot-name", textContent: u ? u.name : e.id }),
        el("span", { className: "bzp-stats", textContent: `${e.atk}/${e.hp}/${e.def}` + (u && u.offering ? ` · ${Math.min(e.deaths || 0, u.offering)}/${u.offering}` : "") }),
      );
      if (selling.has(e)) handle.append(el("span", { className: "bzp-tag", textContent: "à vendre" }));
      handle.dataset.id = e.id;
      handle.dataset.list = list;
      handle.dataset.index = String(i);
      handle.setAttribute("role", "button");
      handle.setAttribute("aria-label", `${u ? u.name : e.id}, ${list === "board" ? "terrain" : "réserve"} ${i + 1}`);
      slot.append(handle);
      return slot;
    });
  }

  function statInput(e, key, min, label, icon) {
    const input = el("input", { type: "number", min: String(min), max: "999", value: String(e[key] ?? 0) });
    input.setAttribute("aria-label", label);
    input.addEventListener("input", () => {
      const v = Math.floor(Number(input.value));
      if (!Number.isFinite(v) || v < min || v > 999) return;
      e[key] = v;
      save();
      update({ editor: false });
    });
    return el("label", { className: "stat" }, el("span", { textContent: icon }), input);
  }

  function renderEditor() {
    const box = $("#bzp-editor");
    const e = selected;
    const list = e ? (me.board.includes(e) ? "board" : me.reserve.includes(e) ? "reserve" : null) : null;
    if (!e || !list) {
      selected = null;
      box.hidden = true;
      return;
    }
    const u = unitById(e.id);
    const sell = el("button", { type: "button", className: selling.has(e) ? "primary" : "", textContent: selling.has(e) ? "✓ Vendue dans ce message" : `Vendre (+${rules.sell_value}🙏)` });
    sell.addEventListener("click", () => {
      if (selling.has(e)) selling.delete(e);
      else selling.add(e);
      update();
    });
    const remove = el("button", { type: "button", className: "ghost", textContent: "Retirer sans vendre" });
    remove.title = "Pour corriger votre état (unité anéantie, perdue…) : rien n'est écrit dans le message.";
    remove.addEventListener("click", () => {
      me[list].splice(me[list].indexOf(e), 1);
      selling.delete(e);
      selected = null;
      save();
      update();
    });
    const close = el("button", { type: "button", className: "ghost", textContent: "Fermer" });
    close.addEventListener("click", () => {
      selected = null;
      update();
    });
    const stats = el("div", { className: "slot-editor bzp-edit-stats" }, statInput(e, "atk", 0, "Attaque", "⚔"), statInput(e, "hp", 1, "Points de vie", "❤"), statInput(e, "def", 0, "Défense", "🛡"));
    if (u && u.offering) stats.append(statInput(e, "deaths", 0, `Morts de l'Offrande (sur ${u.offering})`, "⚰️"));
    box.replaceChildren(
      el("h3", { textContent: `${e.star ? "⭐" : ""}${u ? `${u.emoji} ${u.name}` : e.id}` }),
      el("p", { className: "hint", textContent: "Stats actuelles, bonus permanents compris (Vigueur, Bénédiction, Rancune…)." }),
      stats,
      el("div", { className: "buttons" }, ...(inCompte() ? [] : [sell]), remove, close),
    );
    box.hidden = false;
  }

  function renderPost() {
    const r = result;
    const o = orders();
    const parts = [];
    if (buy) parts.push(`acheter ${unitText(buy.unit)} (niveau ${buy.level})`);
    for (const e of selling) parts.push(`vendre ${unitText(e.id)}`);
    if (o.invest) parts.push(`investir ${o.invest}🙏`);
    $("#bzp-orders").textContent = parts.length ? parts.join(" · ") : "passer (aucun achat : le message fait seulement avancer la page)";
    messages($("#bzp-post-messages"), r.errors || [], r.ok ? r.notes : []);
    if (r.ok) {
      const pay = r.fee + r.price + r.invested;
      $("#bzp-cost").textContent =
        `Ce message coûte ${pay}🙏` +
        (r.sales ? `, les ventes rapportent ${r.sales}🙏` : "") +
        ` : il vous restera ${r.state.favors}🙏.`;
    } else $("#bzp-cost").textContent = "";
    $("#bzp-text").value = r.ok ? r.text : "";
    $("#bzp-copy").hidden = !!API; // with the server, a post is confirmed before it is posted
    $("#bzp-preview").innerHTML = r.ok ? renderTwino(r.text) : "";
    const name = (me.name || "").trim();
    const confirm = $("#bzp-confirm");
    confirm.disabled = !r.ok || !API || !name || confirming;
    confirm.title = !API ? "Le Bazar en ligne n'est pas encore ouvert" : !name ? "Indiquez d'abord votre nom sur le forum" : "";
    $("#bzp-name-hint").hidden = !!name;
  }

  function renderMine() {
    // My account's posts; the server says which were cancelled, by me or by a GM.
    const list = myPosts.slice().sort((a, b) => b.seq - a.seq);
    const live = list.filter((p) => !p.cancelled);
    $("#bzp-mine").replaceChildren(
      ...(list.length
        ? list.map((p) => {
            const c = p.cancelled;
            const li = el(
              "li",
              { className: c ? "undone" : "" },
              el("span", { className: "j-when", textContent: time(p.at) }),
              el("span", { className: "j-by", textContent: `n°${p.seq}` + (p.page ? ` · p. ${p.page}` : "") }),
              el("span", { className: "j-what", textContent: firstLine(p.forum) + (c ? ` · annulé${c.gm ? ` par ${c.by} (MJ)` : ""}` : "") }),
            );
            const buttons = el("span", { className: "buttons" });
            const again = el("button", { type: "button", className: "ghost j-undo", textContent: "Copier" });
            again.addEventListener("click", (ev) => copyText(p.forum, ev.currentTarget));
            buttons.append(again);
            if (!p.cancelled) {
              const undo = el("button", { type: "button", className: "ghost j-undo", textContent: "Annuler" });
              const later = live.some((q) => q.seq > p.seq);
              undo.disabled = !API || later;
              if (later) undo.title = "Annulez d'abord vos messages plus récents";
              undo.addEventListener("click", () => cancelPost(p, undo));
              buttons.append(undo);
            }
            li.append(buttons);
            return li;
          })
        : [el("li", { className: "hint", textContent: "Aucun message confirmé." })]),
    );
  }

  function renderFeed() {
    const events = (remote && remote.events) || [];
    $("#bzp-feed").replaceChildren(
      ...(events.length
        ? events.slice(0, 20).map((e) =>
            el(
              "li",
              { className: e.type === "post" && e.cancelled ? "undone" : "" },
              el("span", { className: "j-when", textContent: time(e.at) }),
              el("span", { className: "j-by", textContent: e.player + (e.verified ? " ✔" : "") }),
              el(
                "span",
                { className: "j-what", textContent: e.type === "cancel" ? `annule le message n°${e.target}${e.gm ? ` (${e.by}, MJ)` : ""}` : `n°${e.seq} · ${firstLine(e.forum)}` },
              ),
            ),
          )
        : [el("li", { className: "hint", textContent: API ? "Aucun message pour l'instant." : "Le Bazar en ligne n'est pas encore ouvert." })]),
    );
  }

  function renderState() {
    const fields = [
      ["#bzp-name", "name"],
      ["#bzp-favors", "favors"],
      ["#bzp-level", "level"],
      ["#bzp-invested", "invested"],
    ];
    for (const [sel, key] of fields) if (document.activeElement !== $(sel)) $(sel).value = String(me[key]);
    const t = rules.upgrade_threshold[me.level - 1];
    $("#bzp-invested-hint").textContent = t ? `/ ${t}🙏 vers le niveau ${me.level + 1}` : "niveau maximum";
    $("#bzp-invested").disabled = !t;
    if (document.activeElement !== $("#bzp-invest")) $("#bzp-invest").value = String(invest);
    if (pinned && pinned.closest(".bzp-slots")) unpin();
    $("#bzp-board").replaceChildren(...slotsOf("board"));
    $("#bzp-reserve").replaceChildren(...slotsOf("reserve"));
  }

  function render({ editor = true } = {}) {
    if (!rules) return;
    renderGod();
    renderHistory();
    compute();
    renderShop();
    renderState();
    if (editor) renderEditor();
    renderPost();
    renderMine();
    renderFeed();
  }
  const update = (opts) => render(opts);

  // ---------------------------------------------------------------- Confirm and cancel
  let confirming = false;

  async function confirmPost() {
    const r = result;
    if (!r || !r.ok || confirming) return;
    const name = (me.name || "").trim();
    if (!name) return messages($("#bzp-messages"), ["Indiquez votre nom sur le forum, en haut de « Mon dieu »."]);
    confirming = true;
    renderPost();
    const seen = canon(me);
    try {
      await ensureAccount(); // the first confirmed post makes the account
      await flush(); // the god's last changes first
      if (canon(me) !== seen) {
        // Reloaded meanwhile (changed on another device): the post must be checked again.
        throw new Error("Votre dieu vient d'être rechargé : vérifiez votre message, puis confirmez à nouveau.");
      }
      const answer = await api("POST", "/posts", {
        version,
        shop: remote && remote.shop ? remote.shop.id : null,
        buy,
        returned: r.returned,
        invest: r.invested,
        fee: r.fee,
        price: r.price,
        sales: r.sales,
        after: { ...clone(r.state), ...arena(me), name: account.name },
        forum: r.text,
      });
      const ev = answer.event;
      applyView(answer);
      buy = null;
      selling = new Set();
      invest = 0;
      selected = null;
      const copied = await copyText(ev.forum, null);
      messages(
        $("#bzp-messages"),
        [],
        copied ? [] : ["Copie automatique impossible : utilisez « Copier » dans « Mes messages »."],
        [`Message n°${ev.seq} confirmé${copied ? " et copié" : ""} : collez-le maintenant dans le sujet du Bazar.`],
      );
    } catch (e) {
      messages($("#bzp-messages"), [e.message]);
      if (e.stale) await refreshMe();
      else if (e.status === 401) forget(e.message);
    } finally {
      confirming = false;
      await load({ quiet: true });
      render();
      $("#bzp-messages").scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }

  async function cancelPost(p, button) {
    const ask =
      `Annuler votre message n°${p.seq} ?\n` +
      "L'unité achetée retourne en boutique et vos unités vendues vous reviennent : votre dieu revient à son état d'avant ce message.\n" +
      "Pensez à supprimer ou corriger le message sur le forum.";
    if (!window.confirm(ask)) return;
    button.disabled = true;
    try {
      await flush();
      const answer = await api("POST", `/posts/${p.seq}/cancel`, {});
      applyView(answer);
      buy = null;
      messages(
        $("#bzp-messages"),
        [],
        answer.restored ? [] : ["Votre dieu a changé depuis ce message : il n'a pas été remis comme avant. Corrigez-le à la main si besoin."],
        [`Message n°${p.seq} annulé${answer.restored ? " : votre dieu est revenu à son état d'avant" : ""}. Supprimez ou corrigez le message sur le forum.`],
      );
    } catch (e) {
      messages($("#bzp-messages"), [e.message]);
      button.disabled = false;
      if (e.status === 401) forget(e.message);
    }
    await load();
  }

  // ---------------------------------------------------------------- Editing the state
  function addUnit() {
    const id = $("#bzp-add-unit").value;
    const u = unitById(id);
    if (!u) return;
    const star = $("#bzp-add-star").checked && u.star_allowed;
    const e = { id, star, deaths: 0, ...baseStats(u, star) };
    if (me.board.length < rules.board_size) me.board.push(e);
    else if (me.reserve.length < rules.reserve_size) me.reserve.push(e);
    else return messages($("#bzp-state-messages"), ["Terrain et réserve pleins : retirez ou vendez une unité d'abord."]);
    messages($("#bzp-state-messages"), [], [], [`${star ? "⭐" : ""}${u.emoji} ${u.name} ajoutée.`]);
    save();
    update();
  }

  function readPost() {
    const text = $("#bzp-paste").value;
    if (!text.trim()) return;
    const r = Engine.call({ op: "bazar_parse", text });
    if (!r.ok) return messages($("#bzp-state-messages"), r.errors, r.warnings);
    me = { ...r.state, ...arena(me), name: me.name };
    selling = new Set();
    selected = null;
    buy = null;
    save();
    $("#bzp-paste").value = "";
    messages($("#bzp-state-messages"), [], r.warnings, ["État importé depuis le message : vérifiez-le ci-dessus."]);
    update();
  }

  function resetDraft() {
    buy = null;
    selling = new Set();
    invest = 0;
    messages($("#bzp-messages"));
    update();
  }

  // ---------------------------------------------------------------- Drag and drop between slots
  let press = null;
  let drag = null;

  const slotAt = (x, y) => {
    const hit = document.elementFromPoint(x, y);
    const s = hit ? hit.closest(".bzp-slot") : null;
    return s;
  };

  function moveUnit(from, i, to, j) {
    const a = me[from];
    const b = me[to];
    const e = a[i];
    if (!e) return;
    if (from === to) {
      if (j >= a.length) j = a.length - 1;
      a.splice(i, 1);
      a.splice(j, 0, e);
    } else if (b[j]) {
      [a[i], b[j]] = [b[j], a[i]];
    } else {
      const size = to === "board" ? rules.board_size : rules.reserve_size;
      if (b.length >= size) return;
      a.splice(i, 1);
      b.push(e);
    }
    save();
    update();
  }

  function onDown(ev) {
    if (ev.button !== undefined && ev.button !== 0) return;
    const h = ev.target.closest(".bzp-handle");
    if (!ev.target.closest(".bzp-handle, .bzp-offer, #bzp-tip")) unpin();
    if (!h) return;
    press = { h, x: ev.clientX, y: ev.clientY, id: ev.pointerId };
    ev.preventDefault();
  }
  function onMove(ev) {
    if (drag) {
      drag.ghost.style.left = `${ev.clientX}px`;
      drag.ghost.style.top = `${ev.clientY}px`;
      const s = slotAt(ev.clientX, ev.clientY);
      if (s !== drag.over) {
        if (drag.over) drag.over.classList.remove("over");
        if (s) s.classList.add("over");
        drag.over = s;
      }
      return;
    }
    if (press && ev.pointerId === press.id && Math.hypot(ev.clientX - press.x, ev.clientY - press.y) > 4) {
      unpin();
      const ghost = el("div", { className: "drag-ghost", textContent: press.h.querySelector(".slot-emoji").textContent });
      document.body.append(ghost);
      document.body.classList.add("dragging");
      drag = { from: press.h.dataset.list, index: Number(press.h.dataset.index), ghost, over: null };
      press = null;
      onMove(ev);
    }
  }
  function onUp(ev) {
    if (drag) {
      const d = drag;
      drag = null;
      d.ghost.remove();
      document.body.classList.remove("dragging");
      if (d.over) d.over.classList.remove("over");
      const s = slotAt(ev.clientX, ev.clientY);
      if (s) moveUnit(d.from, d.index, s.dataset.list, Number(s.dataset.index));
      return;
    }
    if (press && ev.pointerId === press.id) {
      // A tap: the unit's editor (sell, stats).
      const e = me[press.h.dataset.list][Number(press.h.dataset.index)];
      selected = selected === e ? null : e;
      update();
      if (selected) $("#bzp-editor").scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
    press = null;
  }
  function onCancel() {
    if (drag) {
      drag.ghost.remove();
      if (drag.over) drag.over.classList.remove("over");
      document.body.classList.remove("dragging");
      drag = null;
    }
    press = null;
  }

  // ---------------------------------------------------------------- Start
  function wire() {
    const tab = $("#tab-bazar");
    const tabs = [tab, $("#tab-compte")]; // the god's editor is in one or the other
    for (const t of tabs) t.addEventListener("pointerdown", onDown);
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
    document.addEventListener("pointercancel", onCancel);
    for (const t of tabs) t.addEventListener("keydown", (ev) => {
      const h = ev.target.closest(".bzp-handle");
      if (!h) return;
      const list = h.dataset.list;
      const i = Number(h.dataset.index);
      if (ev.key === "Enter" || ev.key === " ") {
        ev.preventDefault();
        const e = me[list][i];
        selected = selected === e ? null : e;
        update();
      } else if (ev.key === "ArrowLeft" || ev.key === "ArrowRight") {
        ev.preventDefault();
        moveUnit(list, i, list, Math.max(0, i + (ev.key === "ArrowLeft" ? -1 : 1)));
      } else if (ev.key === "ArrowDown" && list === "board") {
        ev.preventDefault();
        moveUnit("board", i, "reserve", me.reserve.length);
      } else if (ev.key === "ArrowUp" && list === "reserve") {
        ev.preventDefault();
        moveUnit("reserve", i, "board", me.board.length);
      }
    });
    // Tooltips on the shop's units and the player's units.
    for (const tt of tabs) {
      tt.addEventListener("pointerover", (ev) => {
        const t = ev.target.closest(".bzp-offer, .bzp-handle");
        if (t && ev.pointerType === "mouse" && !pinned && !drag) showTip(t);
      });
      tt.addEventListener("pointerout", (ev) => ev.pointerType === "mouse" && ev.target.closest(".bzp-offer, .bzp-handle") && hideTip());
      tt.addEventListener("focusin", (ev) => {
        const t = ev.target.closest(".bzp-offer, .bzp-handle");
        if (t && !pinned) showTip(t);
      });
      tt.addEventListener("focusout", hideTip);
    }
    // The arena numbers of the challenge composition, and its copy.
    for (const k of ARENA)
      $(`#acc-${k}`).addEventListener("input", () => {
        const input = $(`#acc-${k}`);
        const v = Math.floor(Number(input.value));
        if (input.value === "" || !Number.isFinite(v) || v < Number(input.min) || v > 9999) return;
        me[k] = v;
        save();
        renderCompo();
      });
    $("#acc-copy").addEventListener("click", (ev) => compoText && copyText(compoText, ev.currentTarget));
    document.addEventListener("scroll", hideTip, { passive: true });
    document.addEventListener("keydown", (ev) => ev.key === "Escape" && unpin());

    $("#bzp-name").addEventListener("input", () => {
      me.name = $("#bzp-name").value.replace(/[[\]{}]/g, "").slice(0, 40);
      save();
      renderPost();
    });
    const num = (sel, key, min, max) =>
      $(sel).addEventListener("input", () => {
        const v = Math.floor(Number($(sel).value));
        if (!Number.isFinite(v) || v < min || v > max()) return;
        me[key] = v;
        if (key === "level" && me.invested >= (rules.upgrade_threshold[v - 1] ?? 1)) me.invested = 0;
        save();
        update();
      });
    num("#bzp-favors", "favors", 0, () => 99999);
    num("#bzp-level", "level", 1, () => rules.bazar_odds.length);
    num("#bzp-invested", "invested", 0, () => (rules.upgrade_threshold[me.level - 1] ?? 1) - 1);
    $("#bzp-invest").addEventListener("input", () => {
      const v = Math.floor(Number($("#bzp-invest").value));
      invest = Number.isFinite(v) && v > 0 ? Math.min(v, 999) : 0;
      update();
    });
    $("#bzp-confirm").addEventListener("click", confirmPost);
    $("#bzp-clear").addEventListener("click", resetDraft);
    $("#bzp-reload").addEventListener("click", load);
    $("#bzp-add").addEventListener("click", addUnit);
    $("#bzp-read").addEventListener("click", readPost);
    $("#bzp-copy").addEventListener("click", (ev) => result && result.ok && copyText(result.text, ev.currentTarget));

    // The shop is reloaded while the tab is open.
    const open = () => (!tab.hidden || !$("#tab-compte").hidden) && document.visibilityState === "visible";
    $("#tabbtn-compte").hidden = !API;
    for (const b of $$(".tabs button"))
      b.addEventListener("click", () => {
        placeGod(b.dataset.tab);
        if (b.dataset.tab === "bazar") load();
        if (b.dataset.tab === "compte") {
          load();
          refreshMe();
        }
      });
    setInterval(() => open() && load(), POLL_MS);
    document.addEventListener("visibilitychange", () => open() && load());
  }

  async function start() {
    try {
      await Engine.load();
    } catch (e) {
      return; // app.js shows the error
    }
    rules = Engine.info.rules;
    me = { ...emptyState(), ...(store.get(KEY, null) || {}) };
    const tiers = [...new Set(Engine.info.roster.map((u) => u.tier))];
    $("#bzp-add-unit").replaceChildren(
      ...tiers.map((t) =>
        el("optgroup", { label: `Rang ${t}` }, ...Engine.info.roster.filter((u) => u.tier === t).map((u) => el("option", { value: u.id, textContent: `${u.emoji} ${u.name}` }))),
      ),
    );
    $("#bzp-level").max = String(rules.bazar_odds.length);
    wire();
    renderAccount();
    placeGod($("#tab-compte").hidden ? "bazar" : "compte");
    render();
    if (token) refreshMe();
    if (!$("#tab-bazar").hidden || !$("#tab-compte").hidden) load();
  }

  start();
})();
