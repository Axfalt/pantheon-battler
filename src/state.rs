//! Mutable game state: units owned by players, and the players themselves.

use crate::data::{Data, Keyword, Synergy};
use rand::Rng;

pub type GameRng = rand_chacha::ChaCha8Rng;

/// The lobby's shared supply: a fixed number of copies of each unit.
pub struct Pool {
    counts: Vec<u32>,
}

impl Pool {
    pub fn new(data: &Data) -> Pool {
        let counts = data.units.iter().map(|u| data.pool_copies(u.tier)).collect();
        Pool { counts }
    }

    /// Takes a random unit of `tier`, weighted by the copies left; None if the tier is exhausted.
    pub fn draw(&mut self, data: &Data, tier: usize, rng: &mut GameRng) -> Option<usize> {
        let ids = &data.by_tier[tier - 1];
        let total: u32 = ids.iter().map(|&i| self.counts[i]).sum();
        if total == 0 {
            return None;
        }
        let mut roll = rng.gen_range(0..total);
        for &i in ids {
            if roll < self.counts[i] {
                self.counts[i] -= 1;
                return Some(i);
            }
            roll -= self.counts[i];
        }
        unreachable!()
    }

    /// Copies left of every unit, by unit index.
    pub fn counts(&self) -> &[u32] {
        &self.counts
    }

    /// Copies of a unit still in the pool.
    pub fn left(&self, id: usize) -> u32 {
        self.counts[id]
    }

    /// Takes one copy of a given unit, if any is left.
    pub fn take(&mut self, id: usize) -> bool {
        let left = self.counts[id] > 0;
        if left {
            self.counts[id] -= 1;
        }
        left
    }

    pub fn put(&mut self, id: usize) {
        self.counts[id] += 1;
    }

    /// Returns a board unit to the pool: all its copies for a ⭐ unit.
    pub fn put_unit(&mut self, data: &Data, u: &Unit) {
        self.counts[u.id] += if u.merged { data.rules.merge_copies as u32 } else { 1 };
    }
}

/// A unit on a player's board. Stats include every permanent bonus received so far.
#[derive(Debug, Clone)]
pub struct Unit {
    pub id: usize,
    pub atk: i32,
    pub hp: i32,
    pub def: i32,
    /// Number of Rebirths triggered over the whole game (for Salvage).
    pub rebirths: u32,
    /// Offering quest: deaths counted over the whole game, and whether it paid out.
    pub deaths: u32,
    pub quest_done: bool,
    /// ⭐ unit made from merged copies.
    pub merged: bool,
    /// Multiplier of keyword effects (merge_power of its tier for a ⭐ unit, else 1).
    pub power: i32,
}

impl Unit {
    pub fn new(data: &Data, id: usize) -> Unit {
        let d = &data.units[id];
        Unit { id, atk: d.atk, hp: d.hp, def: d.def, rebirths: 0, deaths: 0, quest_done: false, merged: false, power: 1 }
    }

    /// Multiplier applied to keyword effects.
    pub fn mult(&self) -> i32 {
        self.power
    }

    pub fn boost(&mut self, atk: i32, hp: i32, def: i32) {
        self.atk += atk;
        self.hp += hp;
        self.def += def;
    }
}

#[derive(Debug, Clone)]
pub struct Player {
    pub name: String,
    pub strat: usize,
    pub board: Vec<Unit>,
    /// Held units that don't fight, trigger keywords or count for synergies.
    pub reserve: Vec<Unit>,
    pub offers: Vec<usize>,
    pub favors: i32,
    /// Favors credited at the next income (Plunder, Senet).
    pub bonus_favors: i32,
    pub essence: i32,
    /// Shop level, starting at 1.
    pub level: usize,
    /// Favors invested towards the next shop level.
    pub invested: i32,
    pub wins: u32,
    pub losses: u32,
    pub alive: bool,
    pub out_round: Option<usize>,
    pub merges: u32,
    pub rerolls: u32,
    /// Fights fought, and won, with a ⭐ top-tier unit on the board.
    pub top_star_fights: u32,
    pub top_star_wins: u32,
    /// Shop slots rolled, and those left empty because the pool ran out.
    pub slots: u32,
    pub empty_slots: u32,
    /// Merged top-tier units: wins the game when `top_merge_wins` is on.
    pub ascended: bool,
}

impl Player {
    pub fn new(name: String, strat: usize, essence: i32) -> Player {
        Player {
            name,
            strat,
            board: Vec::new(),
            reserve: Vec::new(),
            offers: Vec::new(),
            favors: 0,
            bonus_favors: 0,
            essence,
            level: 1,
            invested: 0,
            wins: 0,
            losses: 0,
            alive: true,
            out_round: None,
            merges: 0,
            rerolls: 0,
            top_star_fights: 0,
            top_star_wins: 0,
            slots: 0,
            empty_slots: 0,
            ascended: false,
        }
    }

    /// Keywords granted by the synergies the board currently meets.
    pub fn synergies<'a>(&self, data: &'a Data) -> Vec<&'a Keyword> {
        board_synergies(data, &self.board)
    }

    pub fn has_synergy(&self, data: &Data, k: impl Fn(&Keyword) -> bool) -> bool {
        self.synergies(data).into_iter().any(k)
    }

    /// Unmerged copies of a unit held, on the board or in reserve.
    pub fn copies(&self, id: usize) -> usize {
        self.board.iter().chain(&self.reserve).filter(|u| u.id == id && !u.merged).count()
    }

    /// Whether the board or the reserve has a free slot.
    pub fn has_room(&self, data: &Data) -> bool {
        self.board.len() < data.rules.board_size || self.reserve.len() < data.rules.reserve_size
    }

    /// Puts a new unit on the board if there is room, otherwise in reserve.
    pub fn place(&mut self, data: &Data, u: Unit) {
        if self.board.len() < data.rules.board_size {
            self.board.push(u);
        } else {
            self.reserve.push(u);
        }
    }
}

/// Synergies a board meets.
pub fn met_synergies<'a>(data: &'a Data, board: &[Unit]) -> Vec<&'a Synergy> {
    data.rules
        .synergy
        .iter()
        .filter(|s| board.iter().filter(|u| data.units[u.id].is(s.archetype)).count() >= s.count)
        .collect()
}

/// Keywords granted by the synergies a board meets.
pub fn board_synergies<'a>(data: &'a Data, board: &[Unit]) -> Vec<&'a Keyword> {
    met_synergies(data, board).into_iter().map(|s| &s.keyword).collect()
}

/// Endure, from units or synergies: the best one applies, they don't stack. Returns
/// (essence saved, floor).
pub fn best_endure(data: &Data, board: &[Unit]) -> Option<(i32, i32)> {
    let from_units = board.iter().filter_map(|u| {
        data.units[u.id].keywords.iter().find_map(|k| match *k {
            Keyword::Endure { essence, floor } => Some((essence * u.mult(), floor)),
            _ => None,
        })
    });
    let from_synergies = board_synergies(data, board).into_iter().filter_map(|k| match *k {
        Keyword::Endure { essence, floor } => Some((essence, floor)),
        _ => None,
    });
    from_units.chain(from_synergies).max()
}

/// Essence actually lost from a `hit`, once Endure (if any) has reduced it.
pub fn endured(endure: Option<(i32, i32)>, hit: i32) -> i32 {
    match endure {
        Some((saved, floor)) => (hit - saved).max(floor.min(hit)),
        None => hit,
    }
}

/// Adds the Offering deaths of a fight (indexed by board position) to the units, and marks the
/// quests reached.
pub fn record_deaths(data: &Data, board: &mut [Unit], deaths: &[u32]) {
    for (u, &d) in board.iter_mut().zip(deaths) {
        u.deaths += d;
        for k in &data.units[u.id].keywords {
            if let Keyword::Offering { deaths, .. } = *k {
                u.quest_done |= u.deaths >= deaths;
            }
        }
    }
}

/// Fight lost: Grudge (Rancune) gives its stats for good.
pub fn apply_grudge(data: &Data, board: &mut [Unit]) {
    for u in board {
        let m = u.mult();
        for k in &data.units[u.id].keywords {
            if let Keyword::Grudge { atk, hp, def } = *k {
                u.boost(atk * m, hp * m, def * m);
            }
        }
    }
}
