//! One full game (one forum week): shop phases, pairings, fights, divine essence.

use crate::bot::{self, Strategy};
use crate::data::{Data, Keyword};
use crate::fight::{self, Outcome};
use crate::forum;
use crate::shop;
use crate::state::{self, GameRng, Player, Pool, Unit};
use rand::seq::SliceRandom;

pub struct PlayerResult {
    pub strat: usize,
    pub placement: usize,
    pub level: usize,
    pub board: Vec<usize>,
    pub merges: u32,
    pub rerolls: u32,
}

pub struct GameResult {
    pub players: Vec<PlayerResult>,
    pub rounds_played: usize,
    pub eliminated: usize,
    pub ended_by_merge: bool,
    pub slots: u32,
    pub empty_slots: u32,
    pub top_star_fights: u32,
    pub top_star_wins: u32,
    /// Players who held a ⭐ top-tier unit at the end, with their placement.
    pub top_star_places: Vec<usize>,
}

pub fn run_game(
    data: &Data,
    strats: &[Strategy],
    lobby: &[usize],
    rng: &mut GameRng,
    mut log: Option<&mut Vec<String>>,
    forum: bool,
) -> GameResult {
    let rules = &data.rules;
    let mut players: Vec<Player> = lobby
        .iter()
        .enumerate()
        .map(|(i, &s)| Player::new(format!("J{}", i + 1), s, rules.starting_essence))
        .collect();
    let mut pool = Pool::new(data);
    let mut rounds_played = 0;
    let mut ended_by_merge = false;

    for round in 0..rules.rounds {
        let alive: Vec<usize> = (0..players.len()).filter(|&i| players[i].alive).collect();
        if alive.len() <= 1 {
            break;
        }
        rounds_played = round + 1;
        if forum {
            say!(log, "[big][b]Manche {} · Combats[/b][/big]\n", round + 1);
        } else {
            say!(log, "== Manche {} ==", round + 1);
        }

        // The forum output only covers the fights; shop details are for the plain log.
        let mut shop_log = if forum { None } else { log.as_deref_mut() };
        // Shoppers draw from the shared pool one after the other, in a random order.
        let mut shop_order = alive.clone();
        shop_order.shuffle(rng);
        for &i in &shop_order {
            let p = &mut players[i];
            shop::income(data, p);
            shop::roll_offers(data, p, &mut pool, rng);
            if shop_log.is_some() {
                let offers: Vec<&str> = p.offers.iter().map(|&id| data.units[id].emoji.as_str()).collect();
                say!(shop_log, "  {} ({}) ✨{} 🙏{} boutique N{} ({}🙏 investies) : {}", p.name, strats[p.strat].name, p.essence, p.favors, p.level, p.invested, offers.join(" "));
            }
            bot::play(data, &strats[p.strat], p, &mut pool, round, rng, shop_log.as_deref_mut());
            if shop_log.is_some() && !p.reserve.is_empty() {
                let reserve: Vec<&str> = p.reserve.iter().map(|u| data.units[u.id].emoji.as_str()).collect();
                say!(shop_log, "  Réserve de {} : {}", p.name, reserve.join(" "));
            }
            shop::end_of_shop(data, p, &mut pool, rng, shop_log.as_deref_mut());
        }

        // A top-tier merge ends the game before the fights.
        if players.iter().any(|p| p.ascended) {
            if forum {
                for p in players.iter().filter(|p| p.ascended) {
                    say!(log, "[big][b]⭐ Ascension de {} : la partie est gagnée ![/b][/big]", p.name);
                }
            }
            ended_by_merge = true;
            break;
        }

        let mut order = alive.clone();
        order.shuffle(rng);
        for pair in order.chunks(2) {
            let a = pair[0];
            // With an odd count, the last player fights a ghost copy of another board.
            let (b, ghost) = match pair.get(1) {
                Some(&b) => (b, false),
                None => {
                    let others: Vec<usize> = alive.iter().copied().filter(|&i| i != a).collect();
                    (*others.choose(rng).unwrap(), true)
                }
            };
            let mut fight_log = Vec::new();
            let traced = log.is_some().then_some(&mut fight_log);
            let names = [players[a].name.as_str(), players[b].name.as_str()];
            let ((ba, ua), (bb, ub)) = (fighting(data, &players[a]), fighting(data, &players[b]));
            let mut unleashed: Vec<String> = [ua, ub].into_iter().flatten().collect();
            for i in [a, b] {
                if let Some(line) = synergy_line(data, &players[i].name, &players[i].board) {
                    unleashed.push(line);
                }
            }
            let perks = [perks(data, &players[a]), perks(data, &players[b])];
            let res = fight::fight(data, names, &ba, &bb, perks, rng, traced);
            let before = [players[a].essence, players[b].essence];
            let (ra, rb) = match res.outcome {
                Outcome::WinA => (Some(true), Some(false)),
                Outcome::WinB => (Some(false), Some(true)),
                Outcome::Draw => (None, None),
            };
            let hit = |loser_side: usize| match res.outcome {
                Outcome::Draw => data.draw_loss(round),
                _ => data.defeat_loss(&res.survivors[1 - loser_side]),
            };
            apply(data, &mut players[a], ra, &res.rebirths[0], &res.deaths[0], res.essence[0], res.favors[0], hit(0));
            if !ghost {
                apply(data, &mut players[b], rb, &res.rebirths[1], &res.deaths[1], res.essence[1], res.favors[1], hit(1));
            }
            if log.is_some() {
                let (na, nb) = (&players[a].name, &players[b].name);
                let title = format!("{na} vs {nb}{}", if ghost { " (fantôme)" } else { "" });
                if forum {
                    // Line-up in the open, then the fight and its outcome behind spoilers.
                    let mut verdict = vec![match res.outcome {
                        Outcome::WinA => format!("Victoire de {na}"),
                        Outcome::WinB => format!("Victoire de {nb}"),
                        Outcome::Draw => "Match nul".to_string(),
                    }];
                    let sides = if ghost { vec![(a, before[0], res.essence[0], res.favors[0])] } else { vec![(a, before[0], res.essence[0], res.favors[0]), (b, before[1], res.essence[1], res.favors[1])] };
                    for (i, old, back, won_favors) in sides {
                        let now = players[i].essence;
                        let lost = old + back - now;
                        let mut parts = Vec::new();
                        if lost > 0 {
                            parts.push(format!("perd {lost}✨"));
                        }
                        if back > 0 {
                            parts.push(format!("est soigné de {back}✨ (Offrande)"));
                        }
                        if !parts.is_empty() {
                            verdict.push(format!("{} {} (reste {now}✨)", players[i].name, parts.join(" et ")));
                        }
                        if won_favors > 0 {
                            verdict.push(format!("{} gagne {won_favors}🙏 à la manche suivante (Senet)", players[i].name));
                        }
                    }
                    let post = forum::fight_post(&title, &res.start, &unleashed, &fight_log, &verdict.join(" · "), &res.end);
                    if let Some(l) = log.as_deref_mut() {
                        l.extend(post);
                    }
                } else {
                    say!(log, "  {title}");
                    for line in &unleashed {
                        say!(log, "    {line}");
                    }
                    for line in fight_log {
                        say!(log, "{line}");
                    }
                }
            }
        }

        for &i in &alive {
            if players[i].essence <= 0 {
                // The board stays as a record for the report, but its copies return to the pool.
                for u in players[i].board.iter().chain(&players[i].reserve) {
                    pool.put_unit(data, u);
                }
                players[i].alive = false;
                players[i].out_round = Some(round);
                if forum {
                    say!(log, "💀 [b]{}[/b] n'a plus d'essence divine : éliminé.", players[i].name);
                } else {
                    say!(log, "  {} est éliminé", players[i].name);
                }
            }
        }
    }

    // Placement: survivors by essence then wins, then the eliminated, latest first.
    let mut idx: Vec<usize> = (0..players.len()).collect();
    idx.shuffle(rng);
    idx.sort_by_key(|&i| {
        let p = &players[i];
        std::cmp::Reverse((p.ascended, p.alive, p.out_round.map_or(i64::MAX, |r| r as i64), p.essence, p.wins))
    });
    let top_tier = data.by_tier.len() as u8;
    let mut top_star_places = Vec::new();
    let mut results: Vec<PlayerResult> = Vec::with_capacity(players.len());
    if forum {
        say!(log, "\n[big][b]Classement final[/b][/big]");
    }
    for (place, &i) in idx.iter().enumerate() {
        let p = &players[i];
        if p.board.iter().chain(&p.reserve).any(|u| u.merged && data.units[u.id].tier == top_tier) {
            top_star_places.push(place + 1);
        }
        if forum {
            say!(log, "[0] [b]{}[/b] · ✨{} · {}V/{}D", p.name, p.essence.max(0), p.wins, p.losses);
        } else {
            say!(log, "#{} {} ({}) ✨{} {}V/{}D", place + 1, p.name, strats[p.strat].name, p.essence, p.wins, p.losses);
        }
        results.push(PlayerResult {
            strat: p.strat,
            placement: place + 1,
            level: p.level,
            board: p.board.iter().map(|u| u.id).collect(),
            merges: p.merges,
            rerolls: p.rerolls,
        });
    }
    GameResult {
        players: results,
        rounds_played,
        eliminated: players.iter().filter(|p| !p.alive).count(),
        ended_by_merge,
        slots: players.iter().map(|p| p.slots).sum(),
        empty_slots: players.iter().map(|p| p.empty_slots).sum(),
        top_star_fights: players.iter().map(|p| p.top_star_fights).sum(),
        top_star_wins: players.iter().map(|p| p.top_star_wins).sum(),
        top_star_places,
    }
}

/// Applies a fight result: `won` is None on a draw.
fn apply(
    data: &Data,
    p: &mut Player,
    won: Option<bool>,
    rebirths: &[u32],
    deaths: &[u32],
    essence_back: i32,
    favors: i32,
    // Essence this fight costs the player if they lost or drew.
    hit: i32,
) {
    p.essence += essence_back;
    p.bonus_favors += favors;
    state::record_deaths(data, &mut p.board, deaths);
    for (u, &r) in p.board.iter_mut().zip(rebirths) {
        u.rebirths += r;
    }
    let top_tier = data.by_tier.len() as u8;
    if p.board.iter().any(|u| u.merged && data.units[u.id].tier == top_tier) {
        p.top_star_fights += 1;
        p.top_star_wins += (won == Some(true)) as u32;
    }
    // Endure reduces the essence lost on a defeat or a draw.
    let endure = state::best_endure(data, &p.board);
    match won {
        Some(true) => p.wins += 1,
        Some(false) => {
            p.losses += 1;
            p.essence -= state::endured(endure, hit);
            let mut toil = 0;
            for u in &p.board {
                let m = u.mult();
                for k in &data.units[u.id].keywords {
                    match *k {
                        Keyword::Plunder { favors } => p.bonus_favors += favors * m,
                        Keyword::Toil { favors } => toil += favors * m,
                        _ => {}
                    }
                }
            }
            state::apply_grudge(data, &mut p.board);
            shop::add_investment(data, p, toil, None);
        }
        None => p.essence -= state::endured(endure, hit),
    }
}

/// The board as it enters a fight: Unleash (the best one) gives every ally its
/// stats once per fight lost so far, for this fight only. Also returns the log line for it.
fn fighting(data: &Data, p: &Player) -> (Vec<Unit>, Option<String>) {
    let n = p.losses as i32;
    let s = if n > 1 { "s" } else { "" };
    unleash(data, &p.board, n, &p.name, &format!("{n} défaite{s}"))
}

/// Déchaînement: the best Unleash on the board gives every ally its stats `n` times, for this
/// fight only. Returns the board as it enters the fight, and the forum line for it ("Déchaînement :
/// 🦑 Cthulhu (J2) : toute l'armée +2⚔ +2❤ pour ce combat (`why`)"), None when nothing happens.
pub fn unleash(data: &Data, board: &[Unit], n: i32, owner: &str, why: &str) -> (Vec<Unit>, Option<String>) {
    let mut out = board.to_vec();
    let best = board
        .iter()
        .filter_map(|u| {
            data.units[u.id].keywords.iter().find_map(|k| match *k {
                Keyword::Unleash { atk, hp } => Some((u, atk * u.mult(), hp * u.mult())),
                _ => None,
            })
        })
        .max_by_key(|&(_, atk, hp)| atk + hp);
    let Some((u, atk, hp)) = best.filter(|_| n > 0) else { return (out, None) };
    for b in &mut out {
        b.boost(atk * n, hp * n, 0);
    }
    let d = &data.units[u.id];
    let star = if u.merged { "⭐" } else { "" };
    let line = format!(
        "Déchaînement : {star}{} {} ({owner}) : toute l'armée {} pour ce combat ({why})",
        d.emoji,
        d.name,
        crate::data::bonus(atk * n, hp * n, 0)
    );
    (out, Some(line))
}

/// What the player's synergies change in a fight.
pub fn perks(data: &Data, p: &Player) -> fight::Perks {
    board_perks(data, &p.board)
}

/// What a board's synergies change in a fight.
pub fn board_perks(data: &Data, board: &[Unit]) -> fight::Perks {
    let mut perks = fight::Perks::default();
    for k in state::board_synergies(data, board) {
        match *k {
            Keyword::Fortune => perks.fortune = true,
            Keyword::Recurrence => perks.recurrence = true,
            Keyword::Grip { atk } => perks.grip = perks.grip.max(atk),
            _ => {}
        }
    }
    perks
}

/// Short names of the synergies a board meets: "🎲 Fortune", "🐺 Endurance".
pub fn synergy_names(data: &Data, board: &[Unit]) -> Vec<String> {
    state::met_synergies(data, board)
        .into_iter()
        .map(|s| {
            let full = s.keyword.to_string();
            let short = full.split(" (").next().unwrap_or(&full).to_string();
            format!("{} {short}", s.archetype.emoji())
        })
        .collect()
}

/// "Synergies de J2 : 🎲 Fortune · 🐺 Endurance", or None without any.
pub fn synergy_line(data: &Data, name: &str, board: &[Unit]) -> Option<String> {
    let names = synergy_names(data, board);
    (!names.is_empty()).then(|| format!("Synergies de {name} : {}", names.join(" · ")))
}
