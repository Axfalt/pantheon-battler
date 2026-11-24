// Pantheon Battler: the challenge page. The engine and the shared helpers come from core.js
// (window.PB); this script only handles the page.
"use strict";

(function () {
  const { $, $$, el, Engine, store, debounce, copyText, messages, renderTwino, unitById, baseStats, tipBody, placeTip } = window.PB;

  // Share links: the inputs, as base64url JSON in the URL hash.
  function encodeShare(obj) {
    const bytes = new TextEncoder().encode(JSON.stringify(obj));
    let bin = "";
    bytes.forEach((b) => (bin += String.fromCharCode(b)));
    return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }
  function decodeShare(s) {
    const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
    const bin = atob(b64 + "===".slice((b64.length + 3) % 4));
    return JSON.parse(new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0))));
  }

  const shellQuote = (s) => (/^[\w.,@%+=:\/-]+$/.test(s) ? s : "'" + s.replace(/'/g, "'\\''") + "'");

  // ---------------------------------------------------------------- Tabs
  function showTab(name) {
    if (!$("#tab-" + name)) name = "combat";
    for (const b of $$(".tabs button")) b.setAttribute("aria-selected", String(b.dataset.tab === name));
    for (const s of $$(".tab")) s.hidden = s.id !== "tab-" + name;
    store.set("pantheon.tab", name);
  }

  // ---------------------------------------------------------------- Units
  const newEntry = (u, star = false) => ({ id: u.id, star, deaths: 0, ...baseStats(u, star) });

  // Merged mode of the palette: the tiles show, and place, the ⭐ version of each creature.
  let merged = store.get("pantheon.merged", false) === true;
  const mergedOk = (u) => merged && u.star_allowed;

  function setMerged(on) {
    merged = on;
    store.set("pantheon.merged", on);
    const button = $("#merged-toggle");
    button.setAttribute("aria-pressed", String(on));
    $("#palette").classList.toggle("merged", on);
    for (const t of $$(".tile")) {
      const off = on && !unitById(t.dataset.id).star_allowed;
      t.classList.toggle("off", off);
      t.setAttribute("aria-disabled", String(off));
      if (off && selected === t.dataset.id) selectTile(selected);
    }
    unpin();
  }

  function numberInput(value, min, label, onChange) {
    const input = el("input", { type: "number", min: String(min), max: "999", value: String(value) });
    input.setAttribute("aria-label", label);
    input.addEventListener("input", () => {
      const v = Math.max(min, Math.min(999, Math.floor(Number(input.value) || 0)));
      onChange(v);
    });
    input.addEventListener("change", () => (input.value = String(Math.max(min, Math.min(999, Math.floor(Number(input.value) || 0))))));
    return input;
  }

  // ---------------------------------------------------------------- Palette and tooltip
  let selected = null; // unit id picked by a click or a tap, waiting for a slot

  function renderPalette() {
    const info = Engine.info;
    const tiers = [...new Set(info.roster.map((u) => u.tier))];
    $("#palette").replaceChildren(
      ...tiers.map((t) => {
        const units = info.roster.filter((u) => u.tier === t);
        return el(
          "div",
          { className: "tier-box" },
          el("h3", { className: "tier-title", textContent: `Rang ${t} · ${units[0].cost}🙏` }),
          el(
            "div",
            { className: "tiles" },
            ...units.map((u) => {
              const b = el("button", { type: "button", className: "tile", textContent: u.emoji });
              b.dataset.id = u.id;
              b.setAttribute("aria-label", `${u.name}, rang ${u.tier}`);
              b.setAttribute("aria-pressed", "false");
              return b;
            }),
          ),
        );
      }),
    );
    // "Sève (+1 aux Vigueur et Bénédiction)": the name, then what it does.
    $("#synergy-list").replaceChildren(
      ...info.synergies.map((s) => {
        const m = s.keyword.match(/^(.*?)\s*\((.*)\)$/);
        const [name, effect] = m ? [m[1], m[2]] : [s.keyword, ""];
        const n = info.roster.filter((u) => u.archetypes.includes(s.archetype)).length;
        const li = el(
          "li",
          { tabIndex: 0, title: `Montrer les ${n} créatures ${s.archetype} dans la palette` },
          el("span", { className: "syn-arch", textContent: `${s.archetype} ×${s.count} ` }),
          el("strong", { className: "syn-name", textContent: name }),
          el("span", { className: "syn-effect", textContent: effect }),
        );
        li.dataset.archetype = s.archetype;
        li.setAttribute("role", "button");
        li.setAttribute("aria-pressed", "false");
        return li;
      }),
    );
  }

  // A synergy picked in the list: the creatures outside its archetype are greyed out.
  let synergyFilter = null;

  function filterSynergy(archetype) {
    synergyFilter = synergyFilter === archetype ? null : archetype;
    for (const li of $$("#synergy-list li")) li.setAttribute("aria-pressed", String(li.dataset.archetype === synergyFilter));
    $("#palette").classList.toggle("filtered", !!synergyFilter);
    for (const t of $$(".tile")) {
      const member = !synergyFilter || unitById(t.dataset.id).archetypes.includes(synergyFilter);
      t.classList.toggle("dim", !member);
    }
  }

  function selectTile(id) {
    selected = selected === id ? null : id;
    for (const t of $$(".tile")) t.setAttribute("aria-pressed", String(t.dataset.id === selected));
    document.body.classList.toggle("placing", !!selected);
  }

  const tip = () => $("#tip");

  // The unit placed in a slot, for the tooltip of a slot's unit.
  function slotEntry(handle) {
    const slot = handle.closest(".slot");
    return slot ? sideOf(slot.dataset.side).compo.units[Number(slot.dataset.index)] : null;
  }

  let pinned = null; // the tile or slot unit whose tooltip stays open after a click

  function showTip(id, anchor, entry) {
    const u = unitById(id);
    if (!u || dragging) return;
    const box = tip();
    const star = entry ? !!entry.star : anchor.classList.contains("tile") && mergedOk(u);
    box.replaceChildren(
      ...tipBody(u, { star, entry, off: anchor.classList.contains("off"), starHint: " Bouton « ⭐ Fusionnées » pour la voir." }),
    );
    box.hidden = false;
    placeTip(box, anchor);
  }

  // Hover tooltips go away by themselves; a pinned one waits for a click elsewhere or Échap.
  function hideTip() {
    if (!pinned) tip().hidden = true;
  }

  function unpin() {
    if (pinned) pinned.classList.remove("pinned");
    pinned = null;
    tip().hidden = true;
  }

  function togglePin(anchor, id, entry) {
    if (pinned === anchor) {
      unpin();
      return;
    }
    unpin();
    pinned = anchor;
    anchor.classList.add("pinned");
    showTip(id, anchor, entry);
  }

  function tipFor(target) {
    const t = target.closest(".tile, .slot-unit");
    return t ? [t, t.dataset.id, t.classList.contains("slot-unit") ? slotEntry(t) : null] : null;
  }

  // ---------------------------------------------------------------- Players and slots
  const SIZE = () => Engine.info.rules.board_size;
  const emptyCompo = () => ({ name: "", vp: 0, essence: Engine.info.rules.max_essence, streak: 0, wins: 0, units: Array(SIZE()).fill(null) });

  const sides = ["a", "b"].map((key) => {
    const root = $(`.side[data-side="${key}"]`);
    return {
      key,
      root,
      compo: null,
      text: "", // the composition as the forum reads it (format)
      name: $(".p-name", root),
      vp: $(".p-vp", root),
      essence: $(".p-essence", root),
      streak: $(".p-streak", root),
      wins: $(".p-wins", root),
      slots: $(".slots", root),
      status: $(".status", root),
      dice: $$(".dice input", root),
      importBox: $(".import", root),
      importText: $(".import-text", root),
      importMessages: $(".import-messages", root),
    };
  });

  // The composition for the engine: filled slots in order, gaps removed.
  function compact(s) {
    const c = s.compo;
    return { name: c.name, vp: c.vp, essence: c.essence, streak: c.streak, wins: c.wins, units: c.units.filter(Boolean) };
  }

  function refreshSide(s) {
    const c = s.compo;
    let r;
    try {
      r = Engine.call({ op: "format", compo: compact(s) });
    } catch (e) {
      messages(s.status, [e.message]);
      return;
    }
    s.text = r.text || "";
    const count = c.units.filter(Boolean).length;
    const lines = [];
    lines.push(el("p", { className: "summary", textContent: count ? `${count} unité${count > 1 ? "s" : ""}` + (r.synergies && r.synergies.length ? ` · Synergies : ${r.synergies.join(" · ")}` : " · aucune synergie") : "Glissez des créatures dans les emplacements." }));
    const box = el("div");
    const warnings = (r.warnings || []).filter((w) => !/absente|absents/.test(w));
    messages(box, count ? r.errors || [] : [], warnings);
    s.status.replaceChildren(...lines, box);
  }
  const refreshSideSoon = sides.map((s) => debounce(() => refreshSide(s), 120));

  function changed(s, rerender = true) {
    if (rerender) renderSlots(s);
    refreshSideSoon[sides.indexOf(s)]();
    saveCombat();
  }

  function renderSlots(s) {
    const c = s.compo;
    if (pinned && pinned.closest(".slots") === s.slots) unpin();
    s.slots.replaceChildren(
      ...c.units.map((e, i) => {
        const slot = el("div", { className: "slot" + (e ? " full" : "") });
        slot.dataset.side = s.key;
        slot.dataset.index = String(i);
        slot.tabIndex = 0;
        slot.setAttribute("role", "listitem");
        const num = el("span", { className: "slot-num", textContent: String(i + 1) });
        if (!e) {
          slot.setAttribute("aria-label", `Emplacement ${i + 1}, vide`);
          slot.append(num, el("span", { className: "slot-empty", textContent: "Déposez une unité" }));
          return slot;
        }
        const u = unitById(e.id);
        slot.setAttribute("aria-label", `Emplacement ${i + 1} : ${u.name}`);
        const handle = el(
          "div",
          { className: "slot-unit" },
          el("span", { className: "slot-emoji", textContent: (e.star ? "⭐" : "") + u.emoji }),
          el("span", { className: "slot-name", textContent: u.name }),
        );
        handle.dataset.id = u.id;
        const del = el("button", { type: "button", className: "slot-del", textContent: "✕", title: "Retirer" });
        del.addEventListener("click", (ev) => {
          ev.stopPropagation();
          c.units[i] = null;
          changed(s);
        });
        // Editor: stats and quest (⭐ or not comes from the palette).
        const stat = (key, min, label, icon) =>
          el(
            "label",
            { className: "stat" },
            el("span", { textContent: icon }),
            numberInput(e[key], min, label, (v) => {
              e[key] = v;
              changed(s, false);
            }),
          );
        const editor = el(
          "div",
          { className: "slot-editor" },
          stat("atk", 0, "Attaque", "⚔"),
          stat("hp", 1, "Points de vie", "❤"),
          stat("def", 0, "Défense", "🛡"),
        );
        if (u.offering) {
          editor.append(
            el(
              "label",
              { className: "stat offering" },
              el("span", { textContent: "Offrande" }),
              numberInput(Math.min(e.deaths || 0, u.offering), 0, "Morts de l'Offrande", (v) => {
                e.deaths = Math.min(v, u.offering);
                changed(s, false);
              }),
              el("span", { className: "hint", textContent: `/ ${u.offering}` }),
            ),
          );
        }
        slot.append(num, del, handle, editor);
        return slot;
      }),
    );
  }

  function renderPlayer(s) {
    const c = s.compo;
    s.name.value = c.name || "";
    s.vp.value = c.vp ?? 0;
    s.essence.value = c.essence ?? Engine.info.rules.max_essence;
    s.streak.value = c.streak ?? 0;
    s.wins.value = c.wins ?? 0;
  }

  // A composition from the engine (parse, or the one after a fight) into a side's slots.
  function loadInto(s, c) {
    const units = Array(SIZE()).fill(null);
    (c.units || [])
      .filter((e) => e && unitById(e.id))
      .slice(0, SIZE())
      .forEach((e, i) => (units[i] = { id: e.id, star: !!e.star, atk: e.atk, hp: e.hp, def: e.def, deaths: e.deaths || 0 }));
    s.compo = {
      name: c.name || "",
      vp: c.vp ?? 0,
      essence: c.essence ?? Engine.info.rules.max_essence,
      streak: c.streak ?? 0,
      wins: c.wins ?? 0,
      units,
    };
    renderPlayer(s);
    renderSlots(s);
    refreshSide(s);
  }

  function importInto(s, text) {
    if (!text.trim()) {
      messages(s.importMessages, ["Rien à importer : collez d'abord une composition."]);
      return false;
    }
    const p = Engine.call({ op: "parse", text });
    loadInto(s, p.compo);
    const n = p.compo.units.length;
    messages(s.importMessages, p.errors, p.warnings, [`${n} unité${n > 1 ? "s" : ""} importée${n > 1 ? "s" : ""}.`]);
    s.importBox.hidden = true;
    $(".import-toggle", s.root).setAttribute("aria-expanded", "false");
    saveCombat();
    return true;
  }

  // Place a unit (from the palette) in a slot: base stats, ⭐ when the palette shows merged units.
  function place(s, i, id) {
    const u = unitById(id);
    if (!u || (merged && !u.star_allowed)) return;
    s.compo.units[i] = newEntry(u, mergedOk(u));
    changed(s);
  }

  function move(fromSide, fromIndex, toSide, toIndex) {
    const a = fromSide.compo.units;
    const b = toSide.compo.units;
    [a[fromIndex], b[toIndex]] = [b[toIndex], a[fromIndex]];
    changed(fromSide);
    if (toSide !== fromSide) changed(toSide);
  }

  const sideOf = (key) => sides.find((s) => s.key === key);

  // ---------------------------------------------------------------- Drag and drop (pointer events)
  let press = null; // pointer down on a tile or a slot unit, not yet a drag
  let dragging = null; // { source, ghost, over }

  function slotAt(x, y) {
    const hit = document.elementFromPoint(x, y);
    return hit ? hit.closest(".slot") : null;
  }

  function startDrag(ev) {
    const { source } = press;
    unpin();
    const u = unitById(source.id);
    const star = source.kind === "palette" ? mergedOk(u) : !!sideOf(source.side).compo.units[source.index]?.star;
    const ghost = el("div", { className: "drag-ghost", textContent: (star ? "⭐" : "") + u.emoji });
    document.body.append(ghost);
    dragging = { source, ghost, over: null };
    document.body.classList.add("dragging");
    hideTip();
    moveDrag(ev);
  }

  function moveDrag(ev) {
    const d = dragging;
    d.ghost.style.left = `${ev.clientX}px`;
    d.ghost.style.top = `${ev.clientY}px`;
    const slot = slotAt(ev.clientX, ev.clientY);
    if (slot !== d.over) {
      if (d.over) d.over.classList.remove("over");
      if (slot) slot.classList.add("over");
      d.over = slot;
    }
  }

  function endDrag(ev) {
    const d = dragging;
    dragging = null;
    d.ghost.remove();
    document.body.classList.remove("dragging");
    if (d.over) d.over.classList.remove("over");
    const slot = slotAt(ev.clientX, ev.clientY);
    const src = d.source;
    if (slot) {
      const to = sideOf(slot.dataset.side);
      const i = Number(slot.dataset.index);
      if (src.kind === "palette") place(to, i, src.id);
      else if (!(to.key === src.side && i === src.index)) move(sideOf(src.side), src.index, to, i);
    } else if (src.kind === "slot") {
      // Dropped outside every slot: the unit leaves the composition.
      const from = sideOf(src.side);
      from.compo.units[src.index] = null;
      changed(from);
    }
  }

  function onPointerDown(ev) {
    if (ev.button !== undefined && ev.button !== 0) return;
    // A click outside the units closes the tooltip and drops a picked tile.
    if (!ev.target.closest(".tile, .slot, #tip")) {
      unpin();
      if (selected) selectTile(selected);
    }
    const tile = ev.target.closest(".tile:not(.off)");
    const handle = ev.target.closest(".slot-unit");
    if (!tile && !handle) return;
    const source = tile
      ? { kind: "palette", id: tile.dataset.id }
      : { kind: "slot", id: handle.dataset.id, side: handle.closest(".slot").dataset.side, index: Number(handle.closest(".slot").dataset.index) };
    press = { source, x: ev.clientX, y: ev.clientY, target: tile || handle, id: ev.pointerId };
    ev.preventDefault();
  }

  function onPointerMove(ev) {
    if (dragging) {
      moveDrag(ev);
      return;
    }
    if (press && ev.pointerId === press.id && Math.hypot(ev.clientX - press.x, ev.clientY - press.y) > 4) {
      startDrag(ev);
    }
  }

  function onPointerUp(ev) {
    if (dragging) {
      endDrag(ev);
    } else if (press && ev.pointerId === press.id) {
      // A click or a tap without moving: the tooltip stays open, and a tile is picked (a slot
      // clicked next receives it).
      const target = press.target;
      if (press.source.kind === "palette") {
        selectTile(press.source.id);
        togglePin(target, press.source.id);
      } else {
        target.closest(".slot").focus();
        togglePin(target, press.source.id, slotEntry(target));
      }
    }
    press = null;
  }

  function onPointerCancel() {
    if (dragging) {
      dragging.ghost.remove();
      if (dragging.over) dragging.over.classList.remove("over");
      document.body.classList.remove("dragging");
      dragging = null;
    }
    press = null;
  }

  // ---------------------------------------------------------------- Saving and sharing
  function sideState(s) {
    return { c: compact(s), d: s.dice.map((i) => i.value) };
  }
  function setDice(s, d) {
    s.dice.forEach((input, k) => (input.value = (d && d[k]) || ""));
  }
  const saveCombat = debounce(() => store.set("pantheon.combat2", sides.map(sideState)), 300);

  // A saved or shared side: the new form ({c: composition}) or the old one ({t: forum text}).
  function restoreSide(s, st) {
    st = st || {};
    if (st.c) loadInto(s, st.c);
    else if (st.t && st.t.trim()) loadInto(s, Engine.call({ op: "parse", text: st.t }).compo);
    else loadInto(s, emptyCompo());
    if (st.n) {
      s.compo.name = st.n;
      renderPlayer(s);
      refreshSide(s);
    }
    setDice(s, st.d);
  }

  // ---------------------------------------------------------------- Fight
  function readDice() {
    const dice = [];
    const bad = [];
    for (const s of sides) {
      for (const input of s.dice) {
        const v = Number(input.value);
        const ok = input.value.trim() !== "" && Number.isInteger(v) && v >= 1 && v <= 100;
        input.classList.toggle("bad", !ok);
        if (!ok) bad.push(input.getAttribute("aria-label"));
        dice.push(v);
      }
    }
    return { dice, bad };
  }

  let lastFight = null;

  function runFight() {
    const box = $("#run-errors");
    const { dice, bad } = readDice();
    if (bad.length) {
      messages(box, [`Dés manquants ou hors de 1 à 100 : ${bad.join(", ")}`]);
      return;
    }
    const req = { op: "fight", dice };
    let r;
    try {
      sides.forEach((s) => {
        refreshSide(s);
        req[s.key] = { text: s.text };
      });
      r = Engine.call(req);
    } catch (e) {
      messages(box, [e.message]);
      return;
    }
    if (!r.ok) {
      messages(box, r.errors);
      $("#result").hidden = true;
      return;
    }
    messages(box);
    lastFight = r;
    showResult(r);
  }

  // A rain of pixel confetti in the frames' colours, over the whole screen, after a win.
  function confettiRain() {
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const old = $(".confetti-rain");
    if (old) old.remove();
    const rain = el("div", { className: "confetti-rain" });
    rain.setAttribute("aria-hidden", "true");
    document.body.append(rain);
    const colors = ["#f3d867", "#de9d3d", "#8a1a2a", "#b02e36", "#62d4ea", "#90c4ec", "#76c323", "#fdffe6", "#b96c33"];
    const sizes = [[6, 6], [6, 10], [8, 8], [4, 8], [8, 4]]; // whole pixels: crisp squares and strips
    const n = Math.min(140, Math.round(window.innerWidth / 9));
    let last = null;
    for (let k = 0; k < n; k++) {
      const [w, h] = sizes[Math.floor(Math.random() * sizes.length)];
      const piece = el("span", { className: "confetti" });
      piece.style.left = `${Math.random() * 100}%`;
      piece.style.width = `${w}px`;
      piece.style.height = `${h}px`;
      piece.style.background = colors[Math.floor(Math.random() * colors.length)];
      rain.append(piece);
      const fall = window.innerHeight + 40;
      const sway = (Math.random() - 0.5) * 160;
      const turns = (Math.random() < 0.5 ? -1 : 1) * (1 + Math.random() * 3);
      last = piece.animate(
        [
          { transform: "translate(0, 0) rotate(0turn) scaleY(1)" },
          { transform: `translate(${sway * 0.5}px, ${fall * 0.35}px) rotate(${turns * 0.35}turn) scaleY(-1)`, offset: 0.35 },
          { transform: `translate(${-sway * 0.3}px, ${fall * 0.7}px) rotate(${turns * 0.7}turn) scaleY(1)`, offset: 0.7 },
          { transform: `translate(${sway}px, ${fall}px) rotate(${turns}turn) scaleY(-1)` },
        ],
        { duration: 2200 + Math.random() * 1600, delay: Math.random() * 900, easing: "linear", fill: "both" },
      );
    }
    // Everything is gone after the longest fall.
    setTimeout(() => rain.remove(), 4800);
  }

  function showResult(r) {
    $("#result").hidden = false;
    $("#verdict").textContent = r.verdict;
    $("#seed").textContent = String(r.seed);
    // A win is celebrated, a draw is not; the title pops in again at every fight.
    const card = $("#verdict-card");
    card.classList.toggle("draw", r.outcome === "nul");
    card.classList.remove("pop");
    void card.offsetWidth;
    card.classList.add("pop");
    if (r.outcome !== "nul") setTimeout(confettiRain, 250);
    messages($("#fight-warnings"), [], r.warnings);
    $("#forum").value = r.forum;
    $("#preview").innerHTML = renderTwino(r.forum);
    $("#bilan").replaceChildren(...r.bilan.map((line) => el("li", { textContent: line })));
    $$(".after").forEach((box, k) => {
      $("h4", box).textContent = r.names[k] + (r.reset[k] ? " (repart de zéro)" : "");
      $("textarea", box).value = r.after_text[k];
    });
    // The address bar keeps the fight: reloading or sharing the address replays it.
    const share = { a: sideState(sides[0]), b: sideState(sides[1]) };
    try {
      history.replaceState(null, "", "#defi=" + encodeShare(share));
    } catch (e) {
      /* file:// in some browsers */
    }
    $("#result").scrollIntoView({ behavior: "smooth", block: "start" });
  }

  // ---------------------------------------------------------------- Start
  function wire() {
    for (const b of $$(".tabs button")) b.addEventListener("click", () => showTab(b.dataset.tab));

    // Drag and drop, tap-to-place, tooltips.
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("pointermove", onPointerMove);
    document.addEventListener("pointerup", onPointerUp);
    document.addEventListener("pointercancel", onPointerCancel);
    const palette = $("#palette");
    document.addEventListener("pointerover", (ev) => {
      const t = tipFor(ev.target);
      if (t && ev.pointerType === "mouse" && !pinned && !dragging) showTip(t[1], t[0], t[2]);
    });
    document.addEventListener("pointerout", (ev) => {
      if (ev.pointerType === "mouse" && tipFor(ev.target)) hideTip();
    });
    palette.addEventListener("focusin", (ev) => {
      const t = ev.target.closest(".tile");
      if (t && !pinned) showTip(t.dataset.id, t);
    });
    palette.addEventListener("focusout", hideTip);
    palette.addEventListener("keydown", (ev) => {
      const t = ev.target.closest(".tile");
      if (t && !t.classList.contains("off") && (ev.key === "Enter" || ev.key === " ")) {
        ev.preventDefault();
        selectTile(t.dataset.id);
      }
    });
    document.addEventListener("scroll", hideTip, { passive: true });
    document.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        unpin();
        if (selected) selectTile(selected);
      }
    });

    sides.forEach((s) => {
      // A picked tile goes into the slot that is clicked or activated.
      s.slots.addEventListener("click", (ev) => {
        // A placed unit only shows its tooltip: it is replaced by dragging onto it.
        if (ev.target.closest("input, button, label, .slot-unit")) return;
        const slot = ev.target.closest(".slot");
        if (!slot || !selected) return;
        place(s, Number(slot.dataset.index), selected);
        selectTile(selected);
        const again = $(`.slot[data-index="${slot.dataset.index}"]`, s.slots);
        if (again) again.focus();
      });
      s.slots.addEventListener("keydown", (ev) => {
        const slot = ev.target.closest(".slot");
        if (!slot || ev.target !== slot) return;
        const i = Number(slot.dataset.index);
        const units = s.compo.units;
        let to = null;
        if ((ev.key === "Enter" || ev.key === " ") && selected) {
          ev.preventDefault();
          place(s, i, selected);
          selectTile(selected);
          to = i;
        } else if ((ev.key === "ArrowLeft" || ev.key === "ArrowRight") && units[i]) {
          ev.preventDefault();
          const j = i + (ev.key === "ArrowLeft" ? -1 : 1);
          if (j < 0 || j >= units.length) return;
          move(s, i, s, j);
          to = j;
        } else if ((ev.key === "Delete" || ev.key === "Backspace") && units[i]) {
          ev.preventDefault();
          units[i] = null;
          changed(s);
          to = i;
        }
        if (to !== null) $(`.slot[data-index="${to}"]`, s.slots).focus();
      });

      s.name.addEventListener("input", () => {
        s.compo.name = s.name.value;
        changed(s, false);
      });
      for (const [input, key, min] of [[s.vp, "vp", 0], [s.essence, "essence", 1], [s.streak, "streak", 0], [s.wins, "wins", 0]]) {
        input.addEventListener("input", () => {
          s.compo[key] = Math.max(min, Math.min(999, Math.floor(Number(input.value) || 0)));
          changed(s, false);
        });
      }
      $(".copy-side", s.root).addEventListener("click", (ev) => {
        refreshSide(s);
        copyText(s.text, ev.currentTarget);
      });
      const toggle = $(".import-toggle", s.root);
      toggle.addEventListener("click", () => {
        s.importBox.hidden = !s.importBox.hidden;
        toggle.setAttribute("aria-expanded", String(!s.importBox.hidden));
        if (!s.importBox.hidden) s.importText.focus();
      });
      $(".import-go", s.root).addEventListener("click", () => {
        if (importInto(s, s.importText.value)) s.importText.value = "";
      });
      $(".clear-side", s.root).addEventListener("click", () => {
        loadInto(s, emptyCompo());
        saveCombat();
      });

      s.dice.forEach((input, j) => {
        input.addEventListener("input", () => {
          input.classList.remove("bad");
          saveCombat();
        });
        // Pasting "12 3 77" in a die fills the three of them.
        input.addEventListener("paste", (ev) => {
          const text = (ev.clipboardData || window.clipboardData).getData("text");
          const nums = (text.match(/\d+/g) || []).map(Number);
          if (nums.length > 1) {
            ev.preventDefault();
            nums.slice(0, 3 - j).forEach((n, i) => (s.dice[j + i].value = String(n)));
            saveCombat();
          }
        });
      });
    });

    $("#run").addEventListener("click", runFight);
    $("#clear").addEventListener("click", () => {
      sides.forEach((s) => {
        loadInto(s, emptyCompo());
        setDice(s, []);
        messages(s.importMessages);
      });
      $("#result").hidden = true;
      messages($("#run-errors"));
      saveCombat();
      try {
        history.replaceState(null, "", location.pathname + location.search);
      } catch (e) {
        /* ignore */
      }
    });
    $("#copy-forum").addEventListener("click", (ev) => lastFight && copyText(lastFight.forum, ev.currentTarget));
    $("#copy-plain").addEventListener("click", (ev) => lastFight && copyText(lastFight.plain, ev.currentTarget));
    $$(".after").forEach((box, k) => {
      $(".copy-after", box).addEventListener("click", (ev) => lastFight && copyText(lastFight.after_text[k], ev.currentTarget));
      $(".edit-after", box).addEventListener("click", () => {
        if (!lastFight) return;
        loadInto(sides[k], lastFight.after[k]);
        saveCombat();
        sides[k].root.scrollIntoView({ behavior: "smooth", block: "start" });
      });
    });
  }

  async function start() {
    wire();
    const status = $("#engine-status");
    try {
      await Engine.load();
    } catch (e) {
      status.textContent = "Le moteur de combat n'a pas pu démarrer : " + e.message;
      status.classList.add("error");
      return;
    }
    status.hidden = true;
    const info = Engine.info;
    $("#footer").textContent = `Moteur ${info.version} · données ${info.data_hash} · commit ${window.PANTHEON_GIT || "inconnu"}`;
    $("#run").disabled = false;
    renderPalette();
    $("#synergy-list").addEventListener("click", (ev) => {
      const li = ev.target.closest("li[data-archetype]");
      if (li) filterSynergy(li.dataset.archetype);
    });
    $("#synergy-list").addEventListener("keydown", (ev) => {
      const li = ev.target.closest("li[data-archetype]");
      if (li && (ev.key === "Enter" || ev.key === " ")) {
        ev.preventDefault();
        filterSynergy(li.dataset.archetype);
      }
    });
    document.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape" && synergyFilter) filterSynergy(synergyFilter);
    });
    $("#merged-toggle").addEventListener("click", () => setMerged(!merged));
    setMerged(merged);
    const rules = info.rules;
    for (const p of $$(".streak-hint")) {
      p.textContent =
        `💔 toutes les ${rules.comeback_losses} défaites d'affilée : prime de retour +${rules.comeback_favors}🙏 · ` +
        `🔥 dès la ${rules.win_streak_from}e victoire d'affilée : +${rules.win_streak_favors}🙏 par victoire`;
    }
    // The VS is pixel art: drawn at a whole scale of its real size.
    const vs = $("#vs-img");
    const sizeVs = () => vs.naturalWidth && (vs.style.width = `${vs.naturalWidth * 2}px`);
    if (vs.complete) sizeVs();
    else vs.addEventListener("load", sizeVs);

    // A share link wins over the saved inputs; old saves (forum texts) are still read.
    let shared = null;
    const m = location.hash.match(/^#defi=([\w-]+)/);
    if (m) {
      try {
        shared = decodeShare(m[1]);
      } catch (e) {
        messages($("#run-errors"), ["Lien de partage illisible."]);
      }
    }
    const saved = shared ? [shared.a, shared.b] : store.get("pantheon.combat2", null) || store.get("pantheon.combat", null) || [];
    sides.forEach((s, k) => restoreSide(s, saved[k]));
    showTab(shared ? "combat" : store.get("pantheon.tab", "combat"));
    if (shared) runFight();
  }

  start();
})();
