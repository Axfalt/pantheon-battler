// Pantheon Battler: what the public page (app.js) and the GM tools (gm_tool/gm.js) share:
// the Rust engine (WebAssembly, embedded in engine-wasm.js) behind its JSON API, small DOM
// helpers, the Twino preview and the unit tooltip. Exposed as window.PB.
"use strict";

(function () {
  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
  const el = (tag, props = {}, ...children) => {
    const node = document.createElement(tag);
    Object.assign(node, props);
    for (const c of children) node.append(c);
    return node;
  };

  // ---------------------------------------------------------------- Engine
  const Engine = {
    module: null,
    exports: null,
    panic: null,
    info: null,
    loading: null,

    // Several scripts of a page may wait for the engine: it starts once.
    load() {
      if (!this.loading) this.loading = this.start();
      return this.loading;
    },

    async start() {
      if (!window.PANTHEON_WASM) throw new Error("engine-wasm.js est absent ou vide.");
      const bin = Uint8Array.from(atob(window.PANTHEON_WASM), (c) => c.charCodeAt(0));
      this.module = await WebAssembly.compile(bin);
      await this.instantiate();
      this.info = this.call({ op: "info" });
    },

    async instantiate() {
      const self = this;
      const instance = await WebAssembly.instantiate(this.module, {
        env: {
          host_error(ptr, len) {
            self.panic = new TextDecoder().decode(new Uint8Array(self.exports.memory.buffer, ptr, len));
          },
        },
      });
      this.exports = instance.exports;
    },

    // Synchronous call; after a crash the instance is rebuilt in the background.
    call(request) {
      const ex = this.exports;
      if (!ex) throw new Error("Le moteur redémarre, réessayez dans un instant.");
      const input = new TextEncoder().encode(JSON.stringify(request));
      const p = ex.pb_alloc(input.length);
      new Uint8Array(ex.memory.buffer, p, input.length).set(input);
      let r;
      try {
        r = ex.pb_call(p, input.length);
      } catch (e) {
        const why = this.panic || e.message;
        this.panic = null;
        this.exports = null;
        this.instantiate();
        throw new Error("Le moteur a rencontré une erreur interne : " + why);
      }
      ex.pb_free(p, input.length);
      const len = new DataView(ex.memory.buffer).getUint32(r, true);
      const out = new TextDecoder().decode(new Uint8Array(ex.memory.buffer, r + 4, len));
      ex.pb_free(r, len + 4);
      return JSON.parse(out);
    },
  };

  // ---------------------------------------------------------------- Small helpers
  const store = {
    get(key, fallback) {
      try {
        const v = localStorage.getItem(key);
        return v ? JSON.parse(v) : fallback;
      } catch (e) {
        return fallback;
      }
    },
    set(key, value) {
      try {
        localStorage.setItem(key, JSON.stringify(value));
      } catch (e) {
        /* private mode, quota: the page works without it */
      }
    },
  };

  function debounce(fn, ms) {
    let t = null;
    return (...args) => {
      clearTimeout(t);
      t = setTimeout(() => fn(...args), ms);
    };
  }

  async function copyText(text, button) {
    let ok = false;
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(text);
        ok = true;
      }
    } catch (e) {
      ok = false;
    }
    if (!ok) {
      const area = el("textarea", { value: text });
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.append(area);
      area.select();
      try {
        ok = document.execCommand("copy");
      } catch (e) {
        ok = false;
      }
      area.remove();
    }
    if (button) {
      const label = button.dataset.label || button.textContent;
      button.dataset.label = label;
      button.textContent = ok ? "Copié ✓" : "Copie impossible : sélectionnez le texte";
      setTimeout(() => (button.textContent = label), 1600);
    }
    return ok;
  }

  function messages(box, errors = [], warnings = [], oks = []) {
    box.replaceChildren(
      ...errors.map((m) => el("p", { className: "msg error", textContent: m })),
      ...warnings.map((m) => el("p", { className: "msg warn", textContent: m })),
      ...oks.map((m) => el("p", { className: "msg ok", textContent: m })),
    );
  }

  // Twino (BBCode) preview: [b] [i] [big] [*] {hr}, [spoiler] and [collapse=…] blocks, everything else escaped.
  function escapeHtml(s) {
    return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  }
  function renderTwino(text) {
    const inline = (line) =>
      escapeHtml(line)
        .replace(/\[b\]/g, "<strong>")
        .replace(/\[\/b\]/g, "</strong>")
        .replace(/\[i\]/g, "<em>")
        .replace(/\[\/i\]/g, "</em>")
        .replace(/\[big\]/g, '<span class="big">')
        .replace(/\[\/big\]/g, "</span>")
        .replace(/\[\*\]/g, "• ");
    let html = "";
    let depth = 0;
    for (const line of text.split("\n")) {
      const t = line.trim();
      if (t === "[spoiler]") {
        html += "<details><summary>Spoiler</summary>";
        depth++;
      } else if (t === "[/spoiler]") {
        if (depth > 0) {
          html += "</details>";
          depth--;
        }
      } else if (t === "{hr}") {
        html += "<hr>";
      } else {
        // [collapse=Title] at the start of a line, [/collapse] at its end (the engine's layout).
        let rest = line;
        const open = rest.match(/^\s*\[collapse=([^\]]*)\]/);
        if (open) {
          html += `<details class="collapse"><summary>${inline(open[1])}</summary>`;
          depth++;
          rest = rest.slice(open[0].length);
        }
        const close = /\[\/collapse\]\s*$/.test(rest);
        if (close) rest = rest.replace(/\[\/collapse\]\s*$/, "");
        if (rest.trim() || !open) html += inline(rest) + "<br>";
        if (close && depth > 0) {
          html += "</details>";
          depth--;
        }
      }
    }
    while (depth-- > 0) html += "</details>";
    // Unbalanced tags in user text can't leak: the browser closes them inside the preview box.
    return html;
  }

  // ---------------------------------------------------------------- Unit tooltip
  const unitById = (id) => Engine.info.roster.find((u) => u.id === id);

  function baseStats(u, star) {
    const p = star ? u.star_power : 1;
    return { atk: u.atk * p, hp: u.hp * p, def: u.def * p };
  }

  // The tooltip content of a unit: `star` shows its ⭐ version, `entry` the unit placed on a
  // board, `off` a greyed tile, `starHint` what the page offers to see the ⭐ version.
  function tipBody(u, { star = false, entry = null, off = false, starHint = "" } = {}) {
    const base = baseStats(u, star);
    const synergy = (a) => {
      const s = Engine.info.synergies.find((x) => x.archetype === a);
      return s ? `${a} (×${s.count} : ${s.keyword.split(" (")[0]})` : a;
    };
    return [
      el("p", { className: "tip-title" }, el("span", { className: "tip-emoji", textContent: (star ? "⭐" : "") + u.emoji }), ` ${u.name}`),
      el("p", { className: "tip-meta", textContent: `Rang ${u.tier} · ${u.cost}🙏 · ${u.pantheon}` + (star ? " · version fusionnée" : "") }),
      el("p", { className: "tip-stats", textContent: `${star ? "Base ⭐" : "Base"} : ⚔ ${base.atk}   ❤ ${base.hp}   🛡 ${base.def}` }),
      entry
        ? el("p", { className: "tip-here", textContent: `Sur ce terrain : ${entry.star ? "⭐ " : ""}⚔ ${entry.atk}   ❤ ${entry.hp}   🛡 ${entry.def}` + (u.offering ? ` · Offrande ${entry.deaths || 0}/${u.offering}` : "") })
        : "",
      ...((star ? u.details_star : u.details) || []).map((d) =>
        el(
          "div",
          { className: "tip-power" },
          el("p", { className: "tip-power-head" }, el("strong", { textContent: d.name }), el("span", { className: "tip-when", textContent: d.when })),
          el("p", { className: "tip-text", textContent: d.text }),
        ),
      ),
      !u.star_allowed
        ? el("p", { className: "tip-star", textContent: `Rang 3 : fusionner trois exemplaires est une Ascension (+5🏆)${off ? ", il n'a donc pas de version ⭐" : ""}.` })
        : star
          ? el("p", { className: "tip-star", textContent: `⭐ Fusionnée : trois exemplaires réunis, stats de base ×${u.star_power} et nombres de ses pouvoirs doublés.` })
          : el("p", { className: "tip-star", textContent: `Version ⭐ (3 exemplaires) : stats de base ×${u.star_power} et nombres de ses pouvoirs doublés.${starHint}` }),
      el("p", { className: "tip-meta", textContent: u.archetypes.map(synergy).join(" · ") }),
    ];
  }

  // Shows the tooltip box next to its anchor, kept inside the window.
  function placeTip(box, anchor) {
    box.hidden = false;
    const r = anchor.getBoundingClientRect();
    const w = box.offsetWidth;
    const h = box.offsetHeight;
    let x = r.left + r.width / 2 - w / 2;
    x = Math.max(8, Math.min(window.innerWidth - w - 8, x));
    let y = r.bottom + 8;
    if (y + h > window.innerHeight - 8) y = Math.max(8, r.top - h - 8);
    box.style.left = `${x + window.scrollX}px`;
    box.style.top = `${y + window.scrollY}px`;
  }

  window.PB = { $, $$, el, Engine, store, debounce, copyText, messages, escapeHtml, renderTwino, unitById, baseStats, tipBody, placeTip };
})();
