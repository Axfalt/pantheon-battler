//! Shop-phase actions. Each returns whether it happened, so bots can try freely.

use crate::data::{Data, Keyword};
use crate::state::{GameRng, Player, Pool, Unit};
use rand::seq::SliceRandom;
use rand::Rng;

/// Rolls the offers for this round from the player's shop level, out of the shared pool.
pub fn roll_offers(data: &Data, p: &mut Player, pool: &mut Pool, rng: &mut GameRng) {
    p.offers.clear();
    draw_offers(data, p, pool, rng);
}

/// Pays `reroll_cost`: the offers go back to the pool and new ones are drawn, from the current
/// shop level.
pub fn reroll(
    data: &Data,
    p: &mut Player,
    pool: &mut Pool,
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
) -> bool {
    let cost = data.rules.reroll_cost;
    if p.favors < cost {
        return false;
    }
    p.favors -= cost;
    p.rerolls += 1;
    for id in p.offers.drain(..) {
        pool.put(id);
    }
    draw_offers(data, p, pool, rng);
    let offers: Vec<&str> = p.offers.iter().map(|&id| data.units[id].emoji.as_str()).collect();
    say!(log, "  {} relance sa boutique ({cost}🙏) : {}", p.name, offers.join(" "));
    true
}

fn draw_offers(data: &Data, p: &mut Player, pool: &mut Pool, rng: &mut GameRng) {
    for &t in &data.rules.shop_levels[p.level - 1] {
        p.slots += 1;
        match pool.draw(data, t as usize, rng) {
            Some(id) => p.offers.push(id),
            None => p.empty_slots += 1,
        }
    }
}

/// Pours favors into the shop. Each time the invested total reaches the next threshold, the
/// shop goes up a level (new offers from the next round) and the excess carries over.
/// Returns the favors actually invested.
pub fn invest(data: &Data, p: &mut Player, amount: i32, mut log: Option<&mut Vec<String>>) -> i32 {
    let r = &data.rules;
    let max_level = r.shop_levels.len();
    let amount = amount.min(p.favors).min(to_level(data, p, max_level));
    if amount <= 0 {
        return 0;
    }
    p.favors -= amount;
    say!(log, "  {} investit {amount}🙏 dans sa boutique", p.name);
    add_investment(data, p, amount, log);
    amount
}

/// Adds favors to the shop investment without paying for them (Toil).
pub fn add_investment(data: &Data, p: &mut Player, amount: i32, mut log: Option<&mut Vec<String>>) {
    let r = &data.rules;
    let max_level = r.shop_levels.len();
    p.invested += amount;
    while p.level < max_level && p.invested >= r.upgrade_threshold[p.level - 1] {
        p.invested -= r.upgrade_threshold[p.level - 1];
        p.level += 1;
        say!(log, "  La boutique de {} passe au niveau {}", p.name, p.level);
    }
    if p.level >= max_level {
        p.invested = 0;
    }
}

/// Start of shop: income, delayed favors and Hoard interest. A hoarder ignores the cap.
pub fn income(data: &Data, p: &mut Player) {
    let r = &data.rules;
    let interest = p
        .board
        .iter()
        .filter_map(|u| {
            data.units[u.id].keywords.iter().find_map(|k| match *k {
                Keyword::Hoard { per } if per > 0 => Some(u.mult() * (p.favors / per)),
                _ => None,
            })
        })
        .max();
    let total = p.favors + r.base_income + p.bonus_favors + interest.unwrap_or(0);
    p.favors = if interest.is_some() { total } else { total.min(r.favor_cap) };
    p.bonus_favors = 0;
}

/// Favors still to invest before the shop reaches `level`.
pub fn to_level(data: &Data, p: &Player, level: usize) -> i32 {
    let needed: i32 = (p.level..level).map(|l| data.rules.upgrade_threshold[l - 1]).sum();
    (needed - p.invested).max(0)
}

/// Whether buying this unit completes a merge (so it needs no free slot).
pub fn completes_merge(data: &Data, p: &Player, id: usize) -> bool {
    p.copies(id) + 1 >= data.rules.merge_copies
}

pub fn buy(
    data: &Data,
    p: &mut Player,
    pool: &mut Pool,
    offer: usize,
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
) -> bool {
    let id = p.offers[offer];
    if p.favors < data.cost(id) || (!p.has_room(data) && !completes_merge(data, p, id)) {
        return false;
    }
    p.favors -= data.cost(id);
    p.offers.remove(offer);
    p.place(data, Unit::new(data, id));
    let d = &data.units[id];
    say!(log, "  {} achète {} {}", p.name, d.emoji, d.name);
    merge(data, p, pool, rng, log);
    true
}

/// Turns every set of identical unmerged copies into a ⭐ unit: double base stats plus the
/// best permanent bonus among the copies, then a random unit of the next tier as a reward.
/// Merging top-tier units ascends the player instead.
pub fn merge(data: &Data, p: &mut Player, pool: &mut Pool, rng: &mut GameRng, mut log: Option<&mut Vec<String>>) {
    let r = &data.rules;
    while let Some(id) = (0..data.units.len()).find(|&id| p.copies(id) >= r.merge_copies) {
        // Copies are taken from the board first, then from the reserve.
        let mut copies = Vec::new();
        for list in [&mut p.board, &mut p.reserve] {
            let mut i = 0;
            while i < list.len() && copies.len() < r.merge_copies {
                if list[i].id == id && !list[i].merged {
                    copies.push(list.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        let d = &data.units[id];
        // Base stats times the tier's power, plus the best permanent bonus among the copies.
        let best = copies.iter().max_by_key(|u| u.atk + u.hp + u.def).unwrap();
        let power = r.merge_power[d.tier as usize - 1];
        let merged = Unit {
            id,
            atk: d.atk * power + best.atk - d.atk,
            hp: d.hp * power + best.hp - d.hp,
            def: d.def * power + best.def - d.def,
            rebirths: copies.iter().map(|u| u.rebirths).sum(),
            deaths: copies.iter().map(|u| u.deaths).sum(),
            quest_done: copies.iter().any(|u| u.quest_done),
            merged: true,
            power,
        };
        p.place(data, merged);
        p.merges += 1;
        say!(log, "  {} fusionne {} {} en ⭐{} {}", p.name, r.merge_copies, d.name, d.emoji, d.name);
        let next_tier = d.tier as usize; // index of the next tier in by_tier
        if next_tier >= data.by_tier.len() {
            if r.top_merge_wins {
                p.ascended = true;
                say!(log, "  {} accède à l'Ascension : la partie est gagnée !", p.name);
            }
        } else if r.merge_reward {
            let Some(reward) = pool.draw(data, next_tier + 1, rng) else { continue };
            p.place(data, Unit::new(data, reward));
            let ru = &data.units[reward];
            say!(log, "  {} reçoit {} {} pour la fusion", p.name, ru.emoji, ru.name);
        }
    }
}

/// Sells a unit from the board, or from the reserve with `from_reserve`. "When sold" effects
/// trigger either way.
pub fn sell(
    data: &Data,
    p: &mut Player,
    pool: &mut Pool,
    idx: usize,
    from_reserve: bool,
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
) {
    let u = if from_reserve { p.reserve.remove(idx) } else { p.board.remove(idx) };
    pool.put_unit(data, &u);
    let d = &data.units[u.id];
    p.favors += data.rules.sell_value;
    say!(log, "  {} vend {} {}", p.name, d.emoji, d.name);
    let m = u.mult();
    for k in &d.keywords {
        match *k {
            Keyword::Escort { atk, hp } => {
                if let Some(ally) = p.board.choose_mut(rng) {
                    ally.boost(atk * m, hp * m, 0);
                }
            }
            Keyword::Salvage { max } => p.favors += u.rebirths.min(max) as i32 * m,
            _ => {}
        }
    }
}

/// End of shop: Changelings first (a copy can still complete a merge and then grow), then
/// Grow and Bless, all applied at once.
pub fn end_of_shop(
    data: &Data,
    p: &mut Player,
    pool: &mut Pool,
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
) {
    // Offers nobody bought go back to the pool.
    for id in p.offers.drain(..) {
        pool.put(id);
    }

    let is_changeling = |id: usize| data.units[id].has(|k| *k == Keyword::Changeling);
    let fortune = p.has_synergy(data, |k| *k == Keyword::Fortune);
    for i in 1..p.board.len() {
        let (u, target) = (&p.board[i], p.board[i - 1].id);
        if !is_changeling(u.id) || is_changeling(target) {
            continue;
        }
        let name = &data.units[u.id].name;
        let t = &data.units[target];
        // Heads: it becomes a copy, taken from the pool (none left: nothing happens).
        // Tails: it stays as it is.
        let heads = rng.gen_bool(0.5) || (fortune && rng.gen_bool(0.5));
        if heads && pool.take(target) {
            say!(log, "  Le {name} de {} se change en {} {}", p.name, t.emoji, t.name);
            pool.put_unit(data, &p.board[i]);
            p.board[i] = Unit::new(data, target);
        }
    }
    merge(data, p, pool, rng, log);

    let n = p.board.len();
    let mut deltas = vec![(0, 0, 0); n];
    // Sève: +1 in each stat a growth effect raises.
    let sap = p.has_synergy(data, |k| *k == Keyword::Sap) as i32;
    let up = |v: i32, m: i32| if v > 0 { v * m + sap } else { 0 };
    for i in 0..n {
        let m = p.board[i].mult();
        for k in &data.units[p.board[i].id].keywords {
            match *k {
                Keyword::Grow { atk, hp, def } => add(&mut deltas[i], up(atk, m), up(hp, m), up(def, m)),
                Keyword::Bless { atk, hp, all: true } => {
                    for d in deltas.iter_mut() {
                        add(d, up(atk, m), up(hp, m), 0);
                    }
                }
                Keyword::Bless { atk, hp, all: false } if i > 0 => {
                    add(&mut deltas[i - 1], up(atk, m), up(hp, m), 0)
                }
                _ => {}
            }
        }
    }
    for (u, (atk, hp, def)) in p.board.iter_mut().zip(deltas) {
        u.boost(atk, hp, def);
    }
}

fn add(d: &mut (i32, i32, i32), atk: i32, hp: i32, def: i32) {
    d.0 += atk;
    d.1 += hp;
    d.2 += def;
}
