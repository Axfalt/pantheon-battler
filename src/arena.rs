//! Arena mode: no rounds. Each day has 48 half-hour slots. Players come online according to
//! their activity profile, shop in the Bazar (a shared forum thread whose pages are shops) and
//! fight other online players, but only when both agree.

use crate::bazar;
use crate::bot::{self, Strategy};
use crate::data::{Data, Keyword};
use crate::fight::{self, Outcome};
use crate::game;
use crate::shop;
use crate::state::{self, GameRng, Player, Pool, Unit};
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Half-hour slots in a day: the minimum time between two fights of a player.
pub const SLOTS: usize = 48;
/// Fights simulated to estimate a matchup.
const ESTIMATE_FIGHTS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Activity {
    /// One visit of an hour a day.
    Rare,
    /// Three visits of an hour a day.
    Regular,
    /// Online from 8:00 to 23:00.
    Constant,
}

impl Activity {
    pub const ALL: [Activity; 3] = [Activity::Rare, Activity::Regular, Activity::Constant];

    pub fn label(self) -> &'static str {
        match self {
            Activity::Rare => "rare (1 h par jour)",
            Activity::Regular => "régulier (3 × 1 h par jour)",
            Activity::Constant => "constant (8 h – 23 h)",
        }
    }

    fn schedule(self, rng: &mut GameRng) -> Vec<bool> {
        let mut on = vec![false; SLOTS];
        let visit = |on: &mut Vec<bool>, rng: &mut GameRng| {
            let start = rng.gen_range(14..SLOTS - 2);
            on[start] = true;
            on[start + 1] = true;
        };
        match self {
            Activity::Rare => visit(&mut on, rng),
            Activity::Regular => (0..3).for_each(|_| visit(&mut on, rng)),
            Activity::Constant => on[16..46].iter_mut().for_each(|s| *s = true),
        }
        on
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attitude {
    /// Challenges and accepts readily.
    Bold,
    /// Only takes good odds, and stops fighting when clearly ahead.
    Careful,
}

impl Attitude {
    pub const ALL: [Attitude; 2] = [Attitude::Bold, Attitude::Careful];

    pub fn label(self) -> &'static str {
        match self {
            Attitude::Bold => "agressif",
            Attitude::Careful => "prudent (s'arrête s'il mène de 2🏆)",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Seat {
    pub strat: usize,
    pub activity: Activity,
    pub attitude: Attitude,
}

struct Arenist {
    p: Player,
    seat: Seat,
    vp: i32,
    online: Vec<bool>,
    /// Opponent of its last fight: two players can't fight each other twice in a row.
    last_opponent: Option<usize>,
    fights_today: u32,
    wins_today: u32,
    losses_today: u32,
    /// Defeats in a row (Déchaînement, comeback prime) and wins in a row (streak bonus).
    streak: i32,
    win_streak: i32,
    senet_today: i32,
    resets: u32,
    vp_lost: i32,
    fights: u32,
    upsets: u32,
    posts: u32,
    passes: u32,
    ascensions: u32,
    /// Days ended without a fight (each one cost victory points).
    idle_days: u32,
}

/// The Bazar: one shop per level on the current page.
struct Bazar {
    shops: Vec<Vec<usize>>,
    posts: usize,
    /// Author of the last post: nobody may post twice in a row.
    last_poster: Option<usize>,
    pages: u32,
}

pub struct ArenaPlayer {
    pub strat: usize,
    pub activity: Activity,
    pub attitude: Attitude,
    pub place: usize,
    pub vp: i32,
    pub essence: i32,
    pub resets: u32,
    pub vp_lost: i32,
    pub fights: u32,
    pub wins: u32,
    pub upsets: u32,
    pub posts: u32,
    pub level: usize,
    pub ascensions: u32,
    pub idle_days: u32,
    pub board: Vec<usize>,
}

pub struct ArenaResult {
    pub players: Vec<ArenaPlayer>,
    pub pages: u32,
    pub draws: u32,
    /// Leader (most VP) two days before the end, if alone: its attitude and whether it won.
    pub late_leader: Option<(Attitude, bool)>,
}

pub fn run(data: &Data, strats: &[Strategy], seats: &[Seat], rng: &mut GameRng) -> ArenaResult {
    let r = &data.rules;
    let a = &r.arena;
    let mut pool = Pool::new(data);
    let mut ps: Vec<Arenist> = seats
        .iter()
        .enumerate()
        .map(|(i, &seat)| Arenist {
            p: Player::new(format!("J{}", i + 1), seat.strat, r.starting_essence),
            seat,
            vp: 0,
            online: Vec::new(),
            last_opponent: None,
            fights_today: 0,
            wins_today: 0,
            losses_today: 0,
            streak: 0,
            win_streak: 0,
            senet_today: 0,
            resets: 0,
            vp_lost: 0,
            fights: 0,
            upsets: 0,
            posts: 0,
            passes: 0,
            ascensions: 0,
            idle_days: 0,
        })
        .collect();
    let mut bazar = Bazar { shops: Vec::new(), posts: 0, last_poster: None, pages: 0 };
    new_page(data, &mut bazar, &pool, rng);
    let mut cache: HashMap<(u64, u64), (f64, f64)> = HashMap::new();
    let mut draws = 0;
    let mut late_leader = None;
    // Challenges waiting for an answer (challenger, challenged), and those refused today.
    let mut pending: Vec<(usize, usize)> = Vec::new();
    let mut refused: Vec<(usize, usize)> = Vec::new();

    for day in 0..a.days {
        if day + 2 == a.days {
            let top = ps.iter().map(|x| x.vp).max().unwrap_or(0);
            let leaders: Vec<usize> = (0..ps.len()).filter(|&i| ps[i].vp == top).collect();
            if leaders.len() == 1 && top > 0 {
                late_leader = Some(leaders[0]);
            }
        }
        // Daily tick. Challenges last until the end of the day.
        pending.clear();
        refused.clear();
        if day > 0 {
            idle_penalty(data, &mut ps);
        }
        for x in ps.iter_mut() {
            let plunder: i32 = x
                .p
                .board
                .iter()
                .filter(|u| data.units[u.id].has(|k| matches!(k, Keyword::Plunder { .. })))
                .map(|u| u.mult())
                .max()
                .unwrap_or(0);
            x.p.bonus_favors += plunder * a.plunder_per_loss * x.losses_today as i32;
            shop::income(data, &mut x.p);
            shop::end_of_shop(data, &mut x.p, &mut pool, rng, None);
            ascend(data, x, &mut pool);
            x.fights_today = 0;
            x.wins_today = 0;
            x.losses_today = 0;
            x.senet_today = 0;
            x.online = x.seat.activity.schedule(rng);
        }

        for slot in 0..SLOTS {
            let mut online: Vec<usize> = (0..ps.len()).filter(|&i| ps[i].online[slot]).collect();
            online.shuffle(rng);
            for &i in &online {
                shop_visit(data, strats, &mut ps[i], i, &mut bazar, &mut pool, rng);
            }
            // No waiting time between fights: players keep answering and challenging until
            // nothing more happens in this half hour.
            for _ in 0..a.max_fights_per_day {
                let fought: u32 = ps.iter().map(|x| x.fights).sum();
                online.shuffle(rng);
                for &i in &online {
                    // Challenges waiting for this player, oldest first.
                    let waiting: Vec<usize> = pending.iter().filter(|c| c.1 == i).map(|c| c.0).collect();
                    for c in waiting {
                        if let Some(draw) = answer(data, &mut ps, c, i, &mut pool, rng, &mut cache, &mut pending, &mut refused) {
                            draws += draw as u32;
                        }
                    }
                    // New challenges, to anyone it expects to beat (online or not), the best first,
                    // at most 3 waiting at once.
                    if turtles(&ps, i) || ps[i].p.board.is_empty() {
                        continue;
                    }
                    let open = pending.iter().filter(|c| c.0 == i).count();
                    let mut wanted: Vec<(usize, f64)> = Vec::new();
                    for j in 0..ps.len() {
                        if j == i
                            || pending.contains(&(i, j))
                            || refused.contains(&(i, j))
                            || ps[j].p.board.is_empty()
                            || ps[i].last_opponent == Some(j)
                        {
                            continue;
                        }
                        let (win, draw) = estimate(data, &ps[i], &ps[j], &mut cache, rng);
                        let v = value(data, &ps[i], &ps[j], win, draw);
                        if v >= challenge_bar(&ps[i]) {
                            wanted.push((j, v));
                        }
                    }
                    wanted.sort_by(|x, y| y.1.total_cmp(&x.1));
                    for (j, _) in wanted.into_iter().take(3usize.saturating_sub(open)) {
                        pending.push((i, j));
                        // An opponent who is online answers right away.
                        if ps[j].online[slot] {
                            if let Some(draw) = answer(data, &mut ps, i, j, &mut pool, rng, &mut cache, &mut pending, &mut refused) {
                                draws += draw as u32;
                            }
                        }
                    }
                }
                if ps.iter().map(|x| x.fights).sum::<u32>() == fought {
                    break;
                }
            }
        }
    }

    idle_penalty(data, &mut ps);

    // Ranking: victory points, then essence, then fewest resets.
    let mut idx: Vec<usize> = (0..ps.len()).collect();
    idx.shuffle(rng);
    idx.sort_by_key(|&i| std::cmp::Reverse((ps[i].vp, ps[i].p.essence, -(ps[i].resets as i32))));
    let mut players = Vec::new();
    let mut winner = 0;
    for (place, &i) in idx.iter().enumerate() {
        let x = &ps[i];
        if place == 0 {
            winner = i;
        }
        players.push(ArenaPlayer {
            strat: x.seat.strat,
            activity: x.seat.activity,
            attitude: x.seat.attitude,
            place: place + 1,
            vp: x.vp,
            essence: x.p.essence,
            resets: x.resets,
            vp_lost: x.vp_lost,
            fights: x.fights,
            wins: x.p.wins,
            upsets: x.upsets,
            posts: x.posts,
            level: x.p.level,
            ascensions: x.ascensions,
            idle_days: x.idle_days,
            board: x.p.board.iter().map(|u| u.id).collect(),
        });
    }
    ArenaResult {
        players,
        pages: bazar.pages,
        draws,
        late_leader: late_leader.map(|l| (ps[l].seat.attitude, l == winner)),
    }
}

fn can_fight(a: &crate::data::ArenaRules, x: &Arenist) -> bool {
    x.fights_today < a.max_fights_per_day && !x.p.board.is_empty()
}

/// A player who didn't fight during the day loses victory points (never below 0).
fn idle_penalty(data: &Data, ps: &mut [Arenist]) {
    for x in ps.iter_mut() {
        if x.fights_today == 0 {
            x.vp = (x.vp - data.rules.arena.idle_vp_loss).max(0);
            x.idle_days += 1;
        }
    }
}

/// A careful player stops fighting while it leads by 2 VP or more, once it has fought today.
fn turtles(ps: &[Arenist], i: usize) -> bool {
    if ps[i].seat.attitude != Attitude::Careful || ps[i].fights_today == 0 {
        return false;
    }
    let others = (0..ps.len()).filter(|&j| j != i).map(|j| ps[j].vp).max().unwrap_or(0);
    ps[i].vp >= others + 2
}

/// What a fight against `y` is worth to `x`, in victory points: the VP it may win, minus the VP
/// it may lose through a reset (the bigger the expected defeat against its essence, the likelier),
/// plus the idle penalty it avoids if it hasn't fought today.
fn value(data: &Data, x: &Arenist, y: &Arenist, win: f64, draw: f64) -> f64 {
    let a = &data.rules.arena;
    let gain = if y.vp > x.vp { 2.0 } else { 1.0 };
    // About 60 % of the winner's units are still standing at the end of a fight.
    let hit: f64 = y.p.board.iter().map(|u| data.units[u.id].tier as f64 * if u.merged { 2.0 } else { 1.0 }).sum::<f64>()
        * 0.6
        * if x.vp > y.vp { 2.0 } else { 1.0 };
    let essence = x.p.essence.max(1) as f64;
    let stake = x.vp as f64 + 2.0;
    let lose = 1.0 - win - draw;
    let mut v = win * gain - lose * stake * (hit.max(1.0) / essence).min(1.0).powi(2) - draw * stake / (essence * essence);
    if x.fights_today == 0 {
        v += a.idle_vp_loss as f64;
    }
    v
}

/// Value a fight must have for the player to challenge, and to accept.
fn challenge_bar(x: &Arenist) -> f64 {
    match x.seat.attitude {
        Attitude::Bold => 0.1,
        Attitude::Careful => 0.3,
    }
}

fn accept_bar(x: &Arenist) -> f64 {
    match x.seat.attitude {
        Attitude::Bold => -0.3,
        Attitude::Careful => 0.0,
    }
}

/// `j` answers the challenge of `c`, if both may fight now: the fight is played at once, with
/// the boards as they are. Returns None while the challenge has to wait, else whether it ended
/// in a draw (false when refused).
#[allow(clippy::too_many_arguments)]
fn answer(
    data: &Data,
    ps: &mut [Arenist],
    c: usize,
    j: usize,
    pool: &mut Pool,
    rng: &mut GameRng,
    cache: &mut HashMap<(u64, u64), (f64, f64)>,
    pending: &mut Vec<(usize, usize)>,
    refused: &mut Vec<(usize, usize)>,
) -> Option<bool> {
    let a = &data.rules.arena;
    let rematch = ps[c].last_opponent == Some(j) || ps[j].last_opponent == Some(c);
    if !can_fight(a, &ps[j]) || !can_fight(a, &ps[c]) || rematch {
        return None;
    }
    pending.retain(|x| *x != (c, j));
    if a.counter_pick {
        counter_pick(data, ps, c, j, cache, rng);
    }
    let (win_c, draw) = estimate(data, &ps[c], &ps[j], cache, rng);
    if turtles(ps, j) || value(data, &ps[j], &ps[c], 1.0 - win_c - draw, draw) < accept_bar(&ps[j]) {
        refused.push((c, j));
        return Some(false);
    }
    Some(fight(data, ps, c, j, pool, rng))
}

/// The challenged player `j` has seen the challenger's line-up: it orders its own board to give
/// the challenger the least chance to win.
fn counter_pick(data: &Data, ps: &mut [Arenist], c: usize, j: usize, cache: &mut HashMap<(u64, u64), (f64, f64)>, rng: &mut GameRng) {
    let start = ps[j].p.board.clone();
    let mut best = (f64::INFINITY, start.clone());
    for order in permutations(start.len()) {
        ps[j].p.board = order.iter().map(|&k| start[k].clone()).collect();
        let (win_c, draw) = estimate(data, &ps[c], &ps[j], cache, rng);
        let threat = win_c + draw / 2.0;
        if threat < best.0 {
            best = (threat, ps[j].p.board.clone());
        }
    }
    ps[j].p.board = best.1;
}

/// Every order of `n` items.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for rest in permutations(n - 1) {
        for at in 0..=rest.len() {
            let mut p = rest.clone();
            p.insert(at, n - 1);
            out.push(p);
        }
    }
    out
}

/// The board as it enters a fight: Déchaînement counts the current losing streak.
fn fighting(data: &Data, x: &Arenist) -> Vec<Unit> {
    let n = x.streak.min(data.rules.arena.unleash_max);
    game::unleash(data, &x.p.board, n, &x.p.name, "").0
}

fn board_key(data: &Data, x: &Arenist) -> u64 {
    let mut h = DefaultHasher::new();
    for u in &x.p.board {
        (u.id, u.atk, u.hp, u.def, u.power).hash(&mut h);
    }
    x.streak.min(data.rules.arena.unleash_max).hash(&mut h);
    h.finish()
}

/// Chances that `x` beats `y`, and of a draw, from a few simulated fights (cached per boards).
fn estimate(
    data: &Data,
    x: &Arenist,
    y: &Arenist,
    cache: &mut HashMap<(u64, u64), (f64, f64)>,
    rng: &mut GameRng,
) -> (f64, f64) {
    let key = (board_key(data, x), board_key(data, y));
    if let Some(&v) = cache.get(&key) {
        return v;
    }
    let (bx, by) = (fighting(data, x), fighting(data, y));
    let perks = [game::perks(data, &x.p), game::perks(data, &y.p)];
    let (mut win, mut draw) = (0.0, 0.0);
    for _ in 0..ESTIMATE_FIGHTS {
        match fight::fight(data, ["A", "B"], &bx, &by, perks, rng, None).outcome {
            Outcome::WinA => win += 1.0,
            Outcome::Draw => draw += 1.0,
            Outcome::WinB => {}
        }
    }
    let v = (win / ESTIMATE_FIGHTS as f64, draw / ESTIMATE_FIGHTS as f64);
    cache.insert(key, v);
    v
}

/// Plays a fight and applies everything it changes. Returns true on a draw.
fn fight(data: &Data, ps: &mut [Arenist], i: usize, j: usize, pool: &mut Pool, rng: &mut GameRng) -> bool {
    let a = &data.rules.arena;
    let (bi, bj) = (fighting(data, &ps[i]), fighting(data, &ps[j]));
    let perks = [game::perks(data, &ps[i].p), game::perks(data, &ps[j].p)];
    let res = fight::fight(data, ["A", "B"], &bi, &bj, perks, rng, None);
    let (vi, vj) = (ps[i].vp, ps[j].vp);
    for (side, k) in [(0, i), (1, j)] {
        let x = &mut ps[k];
        x.fights += 1;
        x.fights_today += 1;
        x.last_opponent = Some(if k == i { j } else { i });
        x.p.essence += res.essence[side];
        state::record_deaths(data, &mut x.p.board, &res.deaths[side]);
        for (u, &rb) in x.p.board.iter_mut().zip(&res.rebirths[side]) {
            u.rebirths += rb;
        }
        // Senet: favors right away, capped per day.
        let cap = x
            .p
            .board
            .iter()
            .filter_map(|u| data.units[u.id].keywords.iter().find_map(|kw| match *kw {
                Keyword::Senet { max } => Some(max * u.mult()),
                _ => None,
            }))
            .max()
            .unwrap_or(0);
        let gain = res.favors[side].min(cap - x.senet_today).max(0);
        x.senet_today += gain;
        x.p.favors += gain;
    }
    let (w, l, lside) = match res.outcome {
        Outcome::WinA => (i, j, 1),
        Outcome::WinB => (j, i, 0),
        Outcome::Draw => {
            for k in [i, j] {
                ps[k].p.essence -= a.draw_loss;
                check_reset(data, &mut ps[k], pool);
            }
            return true;
        }
    };
    let upset = (if w == i { vi < vj } else { vj < vi }) as i32;
    let x = &mut ps[w];
    x.vp += 1 + upset;
    x.upsets += upset as u32;
    x.p.favors += a.win_favors;
    x.p.wins += 1;
    x.wins_today += 1;
    x.streak = 0;
    x.win_streak += 1;
    if x.win_streak >= a.win_streak_from as i32 {
        x.p.favors += a.win_streak_favors;
    }

    let loss = data.defeat_loss(&res.survivors[1 - lside]) * (1 + upset);
    let y = &mut ps[l];
    y.p.essence -= state::endured(state::best_endure(data, &y.p.board), loss);
    y.p.losses += 1;
    y.losses_today += 1;
    y.streak += 1;
    y.win_streak = 0;
    state::apply_grudge(data, &mut y.p.board);
    if a.comeback_losses > 0 && y.streak % a.comeback_losses as i32 == 0 {
        y.p.favors += a.comeback_favors;
    }
    check_reset(data, y, pool);
    false
}

/// At 0 essence the player starts over: units back to the pool, victory points lost.
fn check_reset(data: &Data, x: &mut Arenist, pool: &mut Pool) {
    if x.p.essence > 0 {
        return;
    }
    for u in x.p.board.iter().chain(&x.p.reserve) {
        pool.put_unit(data, u);
    }
    x.p = Player::new(x.p.name.clone(), x.seat.strat, data.rules.starting_essence);
    x.vp_lost += x.vp;
    x.vp = 0;
    x.resets += 1;
    x.streak = 0;
    x.win_streak = 0;
}

/// A top-tier merge ascends: victory points, and the ⭐ unit's copies go back to the pool.
fn ascend(data: &Data, x: &mut Arenist, pool: &mut Pool) {
    if !x.p.ascended {
        return;
    }
    x.p.ascended = false;
    let top = data.by_tier.len() as u8;
    let found = |list: &Vec<Unit>| list.iter().position(|u| u.merged && data.units[u.id].tier == top);
    let u = if let Some(k) = found(&x.p.board) {
        x.p.board.remove(k)
    } else if let Some(k) = found(&x.p.reserve) {
        x.p.reserve.remove(k)
    } else {
        return;
    };
    pool.put_unit(data, &u);
    x.vp += data.rules.arena.ascension_vp;
    x.ascensions += 1;
}

/// The GM posts the shops of a new Bazar page: each level's shop drawn from the common reserve
/// (tiers by the level's odds, units in proportion to the copies left).
fn new_page(data: &Data, bz: &mut Bazar, pool: &Pool, rng: &mut GameRng) {
    let levels = data.rules.arena.bazar_odds.len();
    bz.shops = (1..=levels).map(|lvl| bazar::draw_shop(data, lvl, pool.counts(), rng).units).collect();
    bz.posts = 0;
    bz.pages += 1;
}

fn post(data: &Data, bz: &mut Bazar, x: &mut Arenist, i: usize, pool: &Pool, rng: &mut GameRng) {
    x.p.favors -= data.rules.arena.post_cost;
    x.posts += 1;
    bz.last_poster = Some(i);
    bz.posts += 1;
    if bz.posts >= data.rules.arena.page_posts {
        new_page(data, bz, pool, rng);
    }
}

/// One visit to the Bazar: buy what is worth it, post empty messages to hurry the page while
/// the board has room or a merge is one copy away, then invest leftovers and arrange the board.
fn shop_visit(
    data: &Data,
    strats: &[Strategy],
    x: &mut Arenist,
    i: usize,
    bz: &mut Bazar,
    pool: &mut Pool,
    rng: &mut GameRng,
) {
    let r = &data.rules;
    let a = &r.arena;
    let s = &strats[x.seat.strat];
    let price = |id: usize| a.post_cost + data.cost(id) - a.post_cost;
    let mut passes = 0;
    // No two posts in a row: one post per visit, unless someone else posted since.
    while bz.last_poster != Some(i) && x.p.favors >= a.post_cost {
        let lvl = x.p.level - 1;
        let p = &x.p;
        let board_room = p.board.len() < r.board_size;
        let budget = p.favors + if p.has_room(data) { 0 } else { r.sell_value };
        let weakest_field = p.board.iter().map(|u| bot::field_score(data, s, u)).fold(f64::INFINITY, f64::min);
        let best = bz.shops[lvl]
            .iter()
            .enumerate()
            .filter(|&(_, &id)| pool.left(id) > 0 && price(id) <= budget)
            .map(|(k, &id)| (k, id, bot::offer_score(data, s, p, id)))
            .filter(|&(_, id, v)| board_room || p.copies(id) > 0 || v > weakest_field * 1.25 + 1.0)
            .max_by(|u, v| u.2.total_cmp(&v.2));
        if let Some((k, id, value)) = best {
            let room = p.has_room(data) || shop::completes_merge(data, p, id);
            if !room {
                // Everything is full: replace the least useful unit held, if clearly worse.
                let held = p.board.iter().map(|u| (false, u)).chain(p.reserve.iter().map(|u| (true, u)));
                let weakest = held
                    .enumerate()
                    .map(|(n, (res, u))| (res, if res { n - p.board.len() } else { n }, bot::keep_score(data, s, p, u)))
                    .min_by(|u, v| u.2.total_cmp(&v.2));
                match weakest {
                    Some((res, n, keep)) if value > keep * 1.25 + 1.0 => {
                        shop::sell(data, &mut x.p, pool, n, res, rng, None);
                    }
                    _ => break,
                }
            }
            if x.p.favors < price(id) {
                break;
            }
            x.p.favors -= data.cost(id) - a.post_cost;
            bz.shops[lvl].remove(k);
            pool.take(id);
            x.p.place(data, Unit::new(data, id));
            shop::merge(data, &mut x.p, pool, rng, None);
            ascend(data, x, pool);
            post(data, bz, x, i, pool, rng);
            continue;
        }
        // Nothing worth buying: an empty post hurries the next page, while it can still buy after.
        let p = &x.p;
        let pair = p.board.iter().chain(&p.reserve).any(|u| !u.merged && p.copies(u.id) + 1 == r.merge_copies);
        if passes < 2 && p.favors >= a.post_cost + r.tier_cost[0] && (p.board.len() < r.board_size || pair) {
            passes += 1;
            x.passes += 1;
            post(data, bz, x, i, pool, rng);
            continue;
        }
        break;
    }
    if s.plan != "low" && x.p.level < s.max_level {
        let amount = (x.p.favors - r.tier_cost[0]).min(shop::to_level(data, &x.p, s.max_level));
        shop::invest(data, &mut x.p, amount, None);
    }
    bot::arrange(data, s, &mut x.p);
}
