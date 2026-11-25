//! The Bazar of the Arena: the shops the GM posts on each new page of the forum topic.
//!
//! Each slot of a level's shop draws a tier from the level's odds, then a unit of that tier in
//! proportion to the copies left in the common reserve (a unit with no copy left never comes
//! up). A shop never shows more copies of a unit than the reserve holds; showing a unit does
//! not take it out of the reserve, only a purchase does.

use crate::data::Data;
use crate::state::GameRng;
use rand::Rng;

/// One level's shop on a Bazar page.
pub struct Shop {
    /// Shop level, from 1.
    pub level: usize,
    /// Units shown (indices into `data.units`), by tier then roster order.
    pub units: Vec<usize>,
    /// Slots whose tier had no copy left and were drawn from the nearest tier instead.
    pub fallbacks: usize,
}

/// The full reserve for `players` registered players: every unit at its tier's copies.
pub fn full_stock(data: &Data, players: u32) -> Vec<u32> {
    data.units.iter().map(|u| data.pool_copies_for(u.tier, players)).collect()
}

/// The nearest tier (lower one first at equal distance) with copies left, from `tier` (0-based).
fn nearest_tier(data: &Data, left: &[u32], tier: usize) -> Option<usize> {
    let has = |t: usize| data.by_tier[t].iter().any(|&i| left[i] > 0);
    let n = data.by_tier.len();
    (0..n).find_map(|d| {
        if tier >= d && has(tier - d) {
            Some(tier - d)
        } else if tier + d < n && has(tier + d) {
            Some(tier + d)
        } else {
            None
        }
    })
}

/// Draws the shop of `level` (from 1) from the reserve `stock` (copies left per unit).
pub fn draw_shop(data: &Data, level: usize, stock: &[u32], rng: &mut GameRng) -> Shop {
    let a = &data.rules.arena;
    let odds = &a.bazar_odds[level - 1];
    let total: u32 = odds.iter().sum();
    let mut left = stock.to_vec();
    let mut units = Vec::new();
    let mut fallbacks = 0;
    for _ in 0..a.bazar_size {
        let roll = rng.gen_range(0..total.max(1));
        let mut acc = 0;
        let tier = odds
            .iter()
            .position(|&o| {
                acc += o;
                roll < acc
            })
            .unwrap_or(0);
        let Some(t) = nearest_tier(data, &left, tier) else {
            break; // the reserve is empty: a shorter shop
        };
        if t != tier {
            fallbacks += 1;
        }
        let ids = &data.by_tier[t];
        let mut pick = rng.gen_range(0..ids.iter().map(|&i| left[i]).sum::<u32>());
        for &i in ids {
            if pick < left[i] {
                left[i] -= 1;
                units.push(i);
                break;
            }
            pick -= left[i];
        }
    }
    units.sort_by_key(|&i| (data.units[i].tier, i));
    Shop { level, units, fallbacks }
}

/// What a unit costs in the Bazar: its own price, and the whole post (post fee included).
pub fn prices(data: &Data, idx: usize) -> (i32, i32) {
    let post = data.rules.arena.post_cost;
    let total = data.cost(idx);
    (total - post, total)
}

/// The forum post (Twino) of a Bazar page: one list per level, then the upgrade costs.
pub fn forum_post(data: &Data, page: Option<u32>, shops: &[Shop]) -> String {
    let a = &data.rules.arena;
    let mut out = vec![match page {
        Some(p) => format!("[big][b]🏪 Bazar · page {p}[/b][/big]"),
        None => "[big][b]🏪 Bazar[/b][/big]".to_string(),
    }];
    out.push(format!(
        "[i]Chaque message coûte {}🙏 en plus du prix de l'unité · un achat par message · pas deux messages d'affilée · nouvelle boutique à la page suivante.[/i]",
        a.post_cost
    ));
    for shop in shops {
        out.push("{hr}".to_string());
        out.push(format!("[b]Niveau {}[/b]", shop.level));
        if shop.units.is_empty() {
            out.push("[i]La réserve commune est vide : rien à vendre.[/i]".to_string());
        }
        for &i in &shop.units {
            let u = &data.units[i];
            let (own, total) = prices(data, i);
            out.push(format!(
                "[*] {} [b]{}[/b] · rang {} · {own}🙏 (message {total}🙏) · ⚔{} ❤{} 🛡{}",
                u.emoji, u.name, u.tier, u.atk, u.hp, u.def
            ));
        }
    }
    // Reminder of the upgrade costs: "5🙏 (niveau 2) · 7🙏 (niveau 3) · 10🙏 (niveau 4)".
    let costs: Vec<String> = data
        .rules
        .upgrade_threshold
        .iter()
        .enumerate()
        .map(|(k, c)| format!("{c}🙏 (niveau {})", k + 2))
        .collect();
    out.push("{hr}".to_string());
    out.push(format!("[i]⬆️ Améliorer sa boutique, en postant investir N au Bazar : {}.[/i]", costs.join(" · ")));
    out.join("\n")
}

/// Warnings for the GM: tiers with no copy left, short shops.
pub fn warnings(data: &Data, shops: &[Shop]) -> Vec<String> {
    let mut w = Vec::new();
    for shop in shops {
        if shop.fallbacks > 0 {
            w.push(format!(
                "Niveau {} : {} emplacement(s) tiré(s) dans un autre rang, faute d'exemplaire dans le rang prévu.",
                shop.level, shop.fallbacks
            ));
        }
        if shop.units.len() < data.rules.arena.bazar_size {
            w.push(format!(
                "Niveau {} : seulement {} unité(s), la réserve commune est presque vide.",
                shop.level,
                shop.units.len()
            ));
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn data() -> Data {
        Data::from_strs(include_str!("../data/units.toml"), include_str!("../data/rules.toml")).unwrap()
    }

    #[test]
    fn same_seed_same_shop() {
        let d = data();
        let stock = full_stock(&d, 6);
        for level in 1..=4 {
            let a = draw_shop(&d, level, &stock, &mut GameRng::seed_from_u64(42));
            let b = draw_shop(&d, level, &stock, &mut GameRng::seed_from_u64(42));
            assert_eq!(a.units, b.units);
            assert_eq!(a.units.len(), d.rules.arena.bazar_size);
        }
    }

    #[test]
    fn never_more_than_the_stock() {
        let d = data();
        let mut stock = full_stock(&d, 6);
        stock[d.by_tier[0][0]] = 0;
        stock[d.by_tier[1][2]] = 1;
        let mut rng = GameRng::seed_from_u64(7);
        for k in 0..5000 {
            let shop = draw_shop(&d, 1 + k % 4, &stock, &mut rng);
            assert!(!shop.units.contains(&d.by_tier[0][0]), "a unit at 0 came up");
            assert!(shop.units.iter().filter(|&&i| i == d.by_tier[1][2]).count() <= 1);
        }
    }

    #[test]
    fn exhausted_tier_falls_back() {
        let d = data();
        let mut stock = full_stock(&d, 6);
        for &i in &d.by_tier[1] {
            stock[i] = 0;
        }
        let shop = draw_shop(&d, 4, &stock, &mut GameRng::seed_from_u64(3));
        assert_eq!(shop.units.len(), d.rules.arena.bazar_size);
        assert!(shop.units.iter().all(|&i| d.units[i].tier != 2));
        assert!(shop.fallbacks > 0);
        let empty = vec![0; stock.len()];
        assert!(draw_shop(&d, 2, &empty, &mut GameRng::seed_from_u64(3)).units.is_empty());
    }

    #[test]
    fn odds_and_stock_weights() {
        let d = data();
        let stock = full_stock(&d, 6);
        let mut rng = GameRng::seed_from_u64(11);
        let level = 3;
        let mut tiers = [0usize; 3];
        let mut n = 0;
        // Within a tier, a unit with twice the copies comes up about twice as often.
        let mut weighted = stock.clone();
        let (lo, hi) = (d.by_tier[0][0], d.by_tier[0][1]);
        weighted[lo] = 2;
        weighted[hi] = 4;
        let (mut c_lo, mut c_hi) = (0, 0);
        for _ in 0..20000 {
            let shop = draw_shop(&d, level, &stock, &mut rng);
            for &i in &shop.units {
                tiers[d.units[i].tier as usize - 1] += 1;
                n += 1;
            }
            let shop = draw_shop(&d, 1, &weighted, &mut rng);
            c_lo += shop.units.iter().filter(|&&i| i == lo).count();
            c_hi += shop.units.iter().filter(|&&i| i == hi).count();
        }
        let odds = &d.rules.arena.bazar_odds[level - 1];
        for t in 0..3 {
            let got = tiers[t] as f64 * 100.0 / n as f64;
            assert!((got - odds[t] as f64).abs() < 2.0, "rank {}: {got:.1} % instead of {} %", t + 1, odds[t]);
        }
        let ratio = c_hi as f64 / c_lo as f64;
        assert!((1.6..2.4).contains(&ratio), "stock 4 vs 2 came up {ratio:.2} times as often");
    }
}
