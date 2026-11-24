//! JSON API shared by the web page (through WebAssembly) and the hidden `api` command, so that
//! both run exactly the same code. Every answer has `ok` and `errors` (French messages).
//!
//! | op | request | answer |
//! |---|---|---|
//! | `info` | — | version, data hash, rules, roster, synergies |
//! | `parse` | `text` | composition, warnings, synergies |
//! | `format` | `compo` | text, warnings |
//! | `fight` | `a`/`b` (`text`, `name`), `dice` | seed, outcome, forum and plain text, updated compositions |

use crate::challenge;
use crate::compo::{self, Composition};
use crate::data::{Data, Keyword};
use crate::game;
use serde_json::{Value, json};

/// Answers one JSON request with one JSON line.
pub fn handle(data: &Data, request: &str) -> String {
    let v: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return fail(vec![format!("Requête illisible : {e}")]).to_string(),
    };
    let answer = match v["op"].as_str() {
        Some("info") => info(data),
        Some("parse") => parse(data, &v),
        Some("format") => format(data, &v),
        Some("fight") => fight(data, &v),
        Some(op) => fail(vec![format!("Opération inconnue : {op}")]),
        None => fail(vec!["Opération manquante (op)".to_string()]),
    };
    answer.to_string()
}

fn fail(errors: Vec<String>) -> Value {
    json!({ "ok": false, "errors": errors })
}

fn info(data: &Data) -> Value {
    let r = &data.rules;
    let roster: Vec<Value> = data
        .units
        .iter()
        .enumerate()
        .map(|(idx, u)| {
            let archetypes: Vec<String> = [Some(u.archetype), u.archetype2].into_iter().flatten().map(|a| format!("{} {a}", a.emoji())).collect();
            json!({
                "id": u.id,
                "name": u.name,
                "emoji": u.emoji,
                "pantheon": u.pantheon.to_string(),
                "archetypes": archetypes,
                "tier": u.tier,
                "cost": data.cost(idx),
                "atk": u.atk,
                "hp": u.hp,
                "def": u.def,
                "keywords": u.keywords.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
                "details": details(r, &u.keywords, 1),
                "details_star": details(r, &u.keywords, r.merge_power[u.tier as usize - 1]),
                "offering": compo::offering_needed(data, idx),
                "star_power": r.merge_power[u.tier as usize - 1],
                "star_allowed": !(r.top_merge_wins && u.tier as usize >= data.by_tier.len()),
            })
        })
        .collect();
    let synergies: Vec<Value> = r
        .synergy
        .iter()
        .map(|s| json!({ "archetype": format!("{} {}", s.archetype.emoji(), s.archetype), "count": s.count, "keyword": s.keyword.to_string() }))
        .collect();
    json!({
        "ok": true,
        "errors": [],
        "version": crate::ENGINE_VERSION,
        "data_hash": data.hash_label(),
        "rules": {
            "board_size": r.board_size,
            "max_essence": r.starting_essence,
            "unleash_max": r.arena.unleash_max,
            "comeback_losses": r.arena.comeback_losses,
            "comeback_favors": r.arena.comeback_favors,
            "win_streak_from": r.arena.win_streak_from,
            "win_streak_favors": r.arena.win_streak_favors,
            "win_favors": r.arena.win_favors,
            "draw_loss": r.arena.draw_loss,
            "loss_per_tier": r.loss_per_tier,
            "merged_factor": r.merged_factor,
            "dice": challenge::DICE,
        },
        "roster": roster,
        "synergies": synergies,
    })
}

/// The powers of a unit explained for the page, at keyword multiplier `m` (2 for a ⭐ unit).
fn details(r: &crate::data::Rules, keywords: &[Keyword], m: i32) -> Vec<Value> {
    keywords
        .iter()
        .map(|k| {
            let (when, text) = k.explain_at(r, m);
            // The Arena pays Pillage per defeat of the day: the amount is in the text, not the name.
            let name = match k {
                Keyword::Plunder { .. } => "Pillage".to_string(),
                _ => k.scaled(m).to_string().split(" (").next().unwrap_or_default().to_string(),
            };
            json!({ "name": name, "when": when, "text": text })
        })
        .collect()
}

/// A composition read from text, with its synergies.
fn parsed_json(data: &Data, p: &compo::Parsed) -> Value {
    let board = p.compo.board(data);
    json!({
        "ok": p.ok(),
        "errors": p.errors,
        "warnings": p.warnings,
        "compo": p.compo,
        "synergies": game::synergy_names(data, &board),
        "data_hash": p.data_hash,
    })
}

fn parse(data: &Data, v: &Value) -> Value {
    let Some(text) = v["text"].as_str() else { return fail(vec!["Texte manquant (text)".to_string()]) };
    parsed_json(data, &compo::parse(data, text))
}

fn format(data: &Data, v: &Value) -> Value {
    let c: Composition = match serde_json::from_value(v["compo"].clone()) {
        Ok(c) => c,
        Err(e) => return fail(vec![format!("Composition illisible : {e}")]),
    };
    let unknown: Vec<String> = c
        .units
        .iter()
        .filter(|e| data.unit_index(&e.id).is_none())
        .map(|e| format!("Unité inconnue : {}", e.id))
        .collect();
    if !unknown.is_empty() {
        return fail(unknown);
    }
    let text = compo::format(data, &c);
    // The text goes through the parser, so the composer shows exactly what a reader will see.
    let mut out = parsed_json(data, &compo::parse(data, &text));
    out["text"] = json!(text);
    out
}

fn fight(data: &Data, v: &Value) -> Value {
    let side = |k: &str| (v[k]["text"].as_str().unwrap_or(""), v[k]["name"].as_str());
    let (a, b) = (side("a"), side("b"));
    let dice: Option<Vec<u32>> = v["dice"]
        .as_array()
        .map(|d| d.iter().map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0)).collect());
    let Some(dice) = dice else { return fail(vec!["Dés manquants (dice)".to_string()]) };
    let c = match challenge::challenge(data, [a.0, b.0], [a.1, b.1], &dice) {
        Ok(c) => c,
        Err(errors) => return fail(errors),
    };
    let s = &c.settlement;
    let outcome = match s.winner {
        Some(0) => "A",
        Some(_) => "B",
        None => "nul",
    };
    json!({
        "ok": true,
        "errors": [],
        "warnings": c.warnings,
        "names": c.names,
        "dice": c.dice,
        "seed": c.seed,
        "outcome": outcome,
        "winner": s.winner.map(|w| c.names[w].clone()),
        "upset": s.upset,
        "verdict": c.verdict(),
        "bilan": s.lines,
        "reset": s.reset,
        "forum": c.forum_text(),
        "plain": c.plain_text(),
        "after": s.after,
        "after_text": s.after.iter().map(|a| compo::format(data, a)).collect::<Vec<_>>(),
        "data_hash": c.data_hash,
        "version": crate::ENGINE_VERSION,
    })
}
