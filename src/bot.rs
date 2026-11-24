//! Simple rule-based players. A strategy is an archetype to favour plus a shop plan.

use crate::data::{Archetype, Data, Keyword, Rules};
use crate::shop;
use crate::state::{GameRng, Player, Pool, Unit};

#[derive(Debug, Clone)]
pub struct Strategy {
    pub name: String,
    /// Archetype whose units the bot prefers; `None` buys on raw value.
    pub focus: Option<Archetype>,
    /// Shop plan key (low / mid / high / rush), shown as bas / moyen / haut / ruée.
    pub plan: &'static str,
    pub max_level: usize,
    pub upgrade_from: usize,
    /// Before this round, an Underdog bot only buys Underdog units, once it fields two units.
    pub sandbag_until: usize,
}

pub fn default_strategies(rules: &Rules) -> Vec<Strategy> {
    let plans = [("low", 1, usize::MAX), ("mid", 3, 1), ("high", 4, 1), ("rush", 4, 0)];
    let plan_name = |plan: &str| match plan {
        "low" => "bas",
        "mid" => "moyen",
        "high" => "haut",
        _ => "ruée",
    };
    let focuses = std::iter::once(None).chain(Archetype::ALL.into_iter().map(Some));
    let mut out = Vec::new();
    for focus in focuses {
        for (plan, max_level, upgrade_from) in plans {
            let label = focus.map_or("Générique".to_string(), |a| a.to_string());
            out.push(Strategy {
                name: format!("{label}-{}", plan_name(plan)),
                focus,
                plan,
                max_level,
                upgrade_from,
                sandbag_until: if focus == Some(Archetype::Underdog) { rules.rounds / 2 } else { 0 },
            });
        }
    }
    out
}

fn keyword_value(k: &Keyword) -> f64 {
    match *k {
        Keyword::Grow { atk, hp, def } => 2.0 * (atk + hp + 2 * def) as f64,
        Keyword::Bless { atk, hp, all } => (atk + hp) as f64 * if all { 5.0 } else { 2.5 },
        Keyword::Legacy => 1.5,
        // About one shared stat per fight on a typical board.
        Keyword::Phalanx { atk, hp } => 1.5 * (atk + hp) as f64,
        Keyword::Escort { atk, hp } => 0.75 * (atk + hp) as f64,
        Keyword::Rebirth => 3.0,
        Keyword::Resurrect => 6.0,
        Keyword::Salvage { .. } => 1.0,
        Keyword::Chaos { dmg } => 1.5 * dmg as f64,
        // About the stats of an average unit on the field, half the time an enemy removed.
        Keyword::Devour => 9.0,
        Keyword::Senet { max } => 0.8 * max as f64,
        Keyword::Chill { amount } => 2.5 * amount as f64,
        Keyword::Petrify => 2.0,
        Keyword::Steal { amount } => 2.0 * amount as f64,
        Keyword::Grudge { atk, hp, def } => 1.5 * (atk + hp + 2 * def) as f64,
        Keyword::Plunder { favors } => 0.75 * favors as f64,
        Keyword::Changeling => 2.5,
        Keyword::Toil { favors } => 1.5 * favors as f64,
        Keyword::Hoard { .. } => 2.0,
        Keyword::Endure { essence, .. } => 2.0 * essence as f64,
        Keyword::Unleash { atk, hp } => 2.5 * (atk + hp) as f64,
        Keyword::Hubris => 1.0,
        Keyword::Taunt => 1.5,
        Keyword::Lifelink => 2.0,
        // Synergies only, never on a unit.
        Keyword::Sap | Keyword::Fortune | Keyword::Grip { .. } => 0.0,
        Keyword::Recurrence => 3.0,
        Keyword::Annihilate => 3.0,
        Keyword::Silence => 2.5,
        Keyword::Dread { atk } => 2.0 * atk as f64,
        // A drawback: it trades itself for its first kill.
        Keyword::Vengeance => -2.0,
        // Worth something only with ⭐ allies, which most boards lack.
        Keyword::LuckyStar { atk, hp } => 0.75 * (atk + hp) as f64,
        Keyword::PotOfGold { favors } => 1.0 * favors as f64,
        Keyword::Thorns { dmg } => 1.5 * dmg as f64,
        Keyword::Offering { essence, .. } => 0.4 * essence as f64,
        Keyword::Raise => 3.0,
    }
}

fn score(data: &Data, s: &Strategy, id: usize, atk: i32, hp: i32, def: i32, mult: i32) -> f64 {
    let d = &data.units[id];
    let mut v = (atk + hp) as f64 + 2.5 * def as f64;
    v += mult as f64 * d.keywords.iter().map(keyword_value).sum::<f64>();
    if s.focus.is_some_and(|a| d.is(a)) {
        v *= 1.5;
    }
    v
}

/// How much the bot wants a shop offer, counting the copies it already owns.
pub fn offer_score(data: &Data, s: &Strategy, p: &Player, id: usize) -> f64 {
    let d = &data.units[id];
    let v = score(data, s, id, d.atk, d.hp, d.def, 1);
    let top_tier = d.tier as usize == data.by_tier.len();
    if shop::completes_merge(data, p, id) {
        if top_tier && data.rules.top_merge_wins { 1000.0 } else { v * 2.5 }
    } else if p.copies(id) > 0 {
        v * 1.3
    } else {
        v
    }
}

/// How much the bot wants a unit on its board.
pub fn field_score(data: &Data, s: &Strategy, u: &Unit) -> f64 {
    score(data, s, u.id, u.atk, u.hp, u.def, u.mult())
}

/// How much the bot wants to keep a unit it holds; Cycle bots happily sell their sell-trigger
/// units, and every bot protects pairs waiting for a merge.
pub fn keep_score(data: &Data, s: &Strategy, p: &Player, u: &Unit) -> f64 {
    let mut v = field_score(data, s, u);
    let sell_trigger = data.units[u.id]
        .has(|k| matches!(k, Keyword::Escort { .. } | Keyword::Salvage { .. }));
    if s.focus == Some(Archetype::Cycle) && sell_trigger {
        v *= 0.5;
    }
    if !u.merged && p.copies(u.id) >= 2 {
        v *= 1.5;
    }
    v
}

pub fn play(
    data: &Data,
    s: &Strategy,
    p: &mut Player,
    pool: &mut Pool,
    round: usize,
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
) {
    let rules = &data.rules;

    // "rush" invests before buying, but keeps enough favors to field two units.
    let climbing = p.level < s.max_level && round >= s.upgrade_from;
    if climbing && s.plan == "rush" {
        let reserve = rules.tier_cost[0] * (2 - p.board.len() as i32).max(0);
        let amount = (p.favors - reserve).min(shop::to_level(data, p, s.max_level));
        shop::invest(data, p, amount, log.as_deref_mut());
    }

    let sandbag = round < s.sandbag_until;
    buy_offers(data, s, p, pool, sandbag, rng, log.as_deref_mut());
    // Reroll while the board has room or a merge is one copy away, keeping enough to buy after.
    let mut rerolls = 0;
    while rerolls < MAX_REROLLS && p.favors >= rules.reroll_cost + rules.tier_cost[0] {
        let pair = p.board.iter().chain(&p.reserve).any(|u| !u.merged && p.copies(u.id) + 1 == rules.merge_copies);
        if p.board.len() >= rules.board_size && !pair {
            break;
        }
        shop::reroll(data, p, pool, rng, log.as_deref_mut());
        rerolls += 1;
        buy_offers(data, s, p, pool, sandbag, rng, log.as_deref_mut());
    }

    // Whatever is left goes into the shop.
    if climbing {
        let amount = shop::to_level(data, p, s.max_level);
        shop::invest(data, p, amount, log.as_deref_mut());
    }

    arrange(data, s, p);
}

/// Fields the best units, then orders them from left to right: tough units first, units that need
/// an ally on their left further right.
pub fn arrange(data: &Data, s: &Strategy, p: &mut Player) {
    let mut held: Vec<Unit> = p.board.drain(..).chain(p.reserve.drain(..)).collect();
    held.sort_by(|a, b| field_score(data, s, b).total_cmp(&field_score(data, s, a)));
    p.reserve = held.split_off(held.len().min(data.rules.board_size));
    p.board = held;

    let front = |u: &Unit| -> i32 {
        let d = &data.units[u.id];
        let mut v = 3 * u.def + u.hp;
        for k in &d.keywords {
            v += match k {
                Keyword::Rebirth | Keyword::Legacy => 4,
                Keyword::Bless { all: false, .. } => -6,
                _ => 0,
            };
        }
        v
    };
    p.board.sort_by_key(|u| std::cmp::Reverse(front(u)));
    // Devour needs an ally on its right.
    let n = p.board.len();
    if n >= 2 && data.units[p.board[n - 1].id].has(|k| *k == Keyword::Devour) {
        p.board.swap(n - 1, n - 2);
    }
    // A Changeling goes just right of the unit worth copying: a pair waiting for its third
    // copy first, otherwise the highest tier.
    let is_changeling = |id: usize| data.units[id].has(|k| *k == Keyword::Changeling);
    let Some(c) = p.board.iter().position(|u| is_changeling(u.id)) else { return };
    let changeling = p.board.remove(c);
    let target = (0..p.board.len())
        .filter(|&i| !is_changeling(p.board[i].id) && !p.board[i].merged)
        .max_by_key(|&i| (p.copies(p.board[i].id), data.units[p.board[i].id].tier));
    let at = target.map_or(p.board.len(), |t| t + 1);
    p.board.insert(at, changeling);
}

const MAX_REROLLS: usize = 2;

/// Buys the best offers it can afford, replacing weaker units once everything is full.
fn buy_offers(
    data: &Data,
    s: &Strategy,
    p: &mut Player,
    pool: &mut Pool,
    sandbag: bool,
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
) {
    let rules = &data.rules;
    loop {
        let board_room = p.board.len() < rules.board_size;
        let budget = p.favors + if p.has_room(data) { 0 } else { rules.sell_value };
        let weakest_field = p.board.iter().map(|u| field_score(data, s, u)).fold(f64::INFINITY, f64::min);
        // With a full board, only buy what prepares a merge or would improve the board.
        let best = p
            .offers
            .iter()
            .enumerate()
            .filter(|&(_, &id)| data.cost(id) <= budget)
            .filter(|&(_, &id)| {
                !sandbag || p.board.len() < 2 || data.units[id].is(Archetype::Underdog)
            })
            .map(|(i, &id)| (i, id, offer_score(data, s, p, id)))
            .filter(|&(_, id, value)| board_room || p.copies(id) > 0 || value > weakest_field * 1.25 + 1.0)
            .max_by(|a, b| a.2.total_cmp(&b.2));
        let Some((offer, id, value)) = best else { break };
        if p.has_room(data) || shop::completes_merge(data, p, id) {
            if !shop::buy(data, p, pool, offer, rng, log.as_deref_mut()) {
                break;
            }
            continue;
        }
        // Everything is full: replace the least useful unit held.
        let held = p.board.iter().map(|u| (false, u)).chain(p.reserve.iter().map(|u| (true, u)));
        let weakest = held
            .enumerate()
            .map(|(k, (in_reserve, u))| {
                let i = if in_reserve { k - p.board.len() } else { k };
                (in_reserve, i, keep_score(data, s, p, u))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        match weakest {
            Some((in_reserve, i, keep)) if value > keep * 1.25 + 1.0 => {
                shop::sell(data, p, pool, i, in_reserve, rng, log.as_deref_mut());
                shop::buy(data, p, pool, offer, rng, log.as_deref_mut());
            }
            _ => break,
        }
    }
}
