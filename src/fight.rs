//! Fight resolution between two boards. Index 0 of a board is its leftmost unit.
//!
//! Start of fight: Devour, then Bonne étoile, then Phalanx, then Petrify, then Emprise (all
//! simultaneous per phase).
//! Phalanx is recounted whenever the board changes: after Chill and after each attack.
//! Then the sides take turns, the one with more units first (a coin flip on a tie). On its
//! turn, a side's leftmost unit that hasn't attacked yet attacks (looping back once all
//! have): Chill, then it strikes a random enemy which strikes back at the same time
//! (Regard de pierre lands with the blow), then Néant and Châtiment for the units killed by
//! the blows, Représailles and Effroi, then Fascination and Chaos, then deaths. Petrify skips a side's next attack. Until one
//! side is empty, or a draw after `max_fight_steps` attacks.

use crate::data::{Data, Keyword};
use crate::state::{GameRng, Unit};
use rand::seq::SliceRandom;
use rand::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    WinA,
    WinB,
    Draw,
}

/// What a side's synergies change during a fight.
#[derive(Clone, Copy, Default)]
pub struct Perks {
    /// Failed coin flips are flipped again once.
    pub fortune: bool,
    /// The first ally to die for good comes back with 1 PV, once.
    pub recurrence: bool,
    /// Every enemy loses this much ATK at the start of the fight.
    pub grip: i32,
}

pub struct FightResult {
    pub outcome: Outcome,
    /// Rebirths triggered, indexed by board position at the start of the fight.
    pub rebirths: [Vec<u32>; 2],
    /// Divine essence each side gets back (Offering).
    pub essence: [i32; 2],
    /// Units destroyed for good (Néant), indexed by board position at the start of the fight.
    pub destroyed: [Vec<bool>; 2],
    /// Favors each side earns for the next round (Senet).
    pub favors: [i32; 2],
    /// Quest deaths counted, indexed by board position at the start of the fight.
    pub deaths: [Vec<u32>; 2],
    /// Units still standing at the end, as (unit id, keyword multiplier).
    pub survivors: [Vec<(usize, i32)>; 2],
    /// Line-ups before the fight and after it (survivors), front units facing each other.
    pub start: String,
    pub end: String,
}

/// A unit's state during one fight; `slot` is its board position when the fight began.
#[derive(Clone)]
struct F {
    slot: usize,
    id: usize,
    atk: i32,
    hp: i32,
    def: i32,
    /// Keyword multiplier (2 for a ⭐ unit).
    mult: i32,
    rebirths_left: i32,
    /// Already attacked in the current pass from left to right.
    attacked: bool,
    /// Offering quest progress, carried over from earlier fights.
    deaths: u32,
    quest_done: bool,
    /// PV before damage: the "PV" stat that Phalanx compares.
    max_hp: i32,
    /// Phalanx bonus currently included in `atk`, and in `max_hp` and `hp`.
    ph_atk: i32,
    ph_hp: i32,
    /// All its powers are cancelled (Regard de pierre).
    silenced: bool,
    /// Killed by a Néant unit: gone for good.
    destroyed: bool,
}

impl F {
    /// Stats as Phalanx compares them: without its own Phalanx bonus, PV before damage.
    fn core(&self) -> (i32, i32, i32) {
        (self.atk - self.ph_atk, self.max_hp - self.ph_hp, self.def)
    }
}

struct Fight<'a> {
    data: &'a Data,
    /// Owner of each side, to tell identical units apart in the log.
    names: [&'a str; 2],
    sides: [Vec<F>; 2],
    /// Attacks each side still has to skip (Petrify).
    petrified: [i32; 2],
    rebirths: [Vec<u32>; 2],
    deaths: [Vec<u32>; 2],
    /// Allies dead during this fight, as they were when they fell (for Raise).
    graveyard: [Vec<F>; 2],
    essence: [i32; 2],
    favors: [i32; 2],
    perks: [Perks; 2],
    /// Éternel retour returns already used (synergy and units with the power together).
    recurred: [i32; 2],
    destroyed: [Vec<bool>; 2],
    log: Option<&'a mut Vec<String>>,
}

pub fn fight<'a>(
    data: &'a Data,
    names: [&'a str; 2],
    a: &[Unit],
    b: &[Unit],
    perks: [Perks; 2],
    rng: &mut GameRng,
    log: Option<&'a mut Vec<String>>,
) -> FightResult {
    let to_f = |board: &[Unit]| -> Vec<F> {
        board
            .iter()
            .enumerate()
            .map(|(slot, u)| F {
                slot,
                id: u.id,
                atk: u.atk,
                hp: u.hp,
                def: u.def,
                mult: u.mult(),
                rebirths_left: 1,
                attacked: false,
                deaths: u.deaths,
                quest_done: u.quest_done,
                max_hp: u.hp,
                ph_atk: 0,
                ph_hp: 0,
                silenced: false,
                destroyed: false,
            })
            .collect()
    };
    let mut f = Fight {
        data,
        names,
        sides: [to_f(a), to_f(b)],
        petrified: [0, 0],
        rebirths: [vec![0; a.len()], vec![0; b.len()]],
        deaths: [vec![0; a.len()], vec![0; b.len()]],
        graveyard: [Vec::new(), Vec::new()],
        essence: [0, 0],
        favors: [0, 0],
        perks,
        recurred: [0, 0],
        destroyed: [vec![false; a.len()], vec![false; b.len()]],
        log,
    };
    let start = f.render();
    for line in start.lines() {
        say!(f.log, "    {line}");
    }
    f.devour(rng);
    f.lucky_star();
    f.phalanx();
    f.petrify();
    f.grip();
    let outcome = f.combat(rng);
    let end = f.render();
    let survivors = [0, 1].map(|s| f.sides[s].iter().map(|u| (u.id, u.mult)).collect());
    FightResult {
        outcome,
        rebirths: f.rebirths,
        essence: f.essence,
        destroyed: f.destroyed,
        favors: f.favors,
        deaths: f.deaths,
        survivors,
        start,
        end,
    }
}

impl<'a> Fight<'a> {
    /// The unit's powers: none once Regard de pierre has cancelled them.
    fn kws(&self, f: &F) -> &'a [Keyword] {
        let data: &'a Data = self.data;
        if f.silenced { &[] } else { &data.units[f.id].keywords }
    }

    fn damage(&self, raw: i32, def: i32) -> i32 {
        (raw - def).max(self.data.rules.min_damage)
    }

    /// "🗿 Ouchebti (J3)": emoji, name and owner.
    fn label(&self, s: usize, f: &F) -> String {
        let d = &self.data.units[f.id];
        let star = if f.mult > 1 { "⭐" } else { "" };
        format!("{star}{} {} ({})", d.emoji, d.name, self.names[s])
    }

    /// "reste 3❤", or "mort".
    fn after(hp: i32) -> String {
        if hp > 0 { format!("reste {hp}❤") } else { "mort".to_string() }
    }

    /// "2⚔ − 1🛡 → 1 dégât, reste 3❤", or "… mort" when the target falls.
    fn hit_text(&self, raw: i32, def: i32, hp_after: i32) -> String {
        let dmg = self.damage(raw, def);
        let calc = if def > 0 { format!("{raw}⚔ − {def}🛡") } else { format!("{raw}⚔") };
        let floor = if raw - def < dmg { " (minimum)" } else { "" };
        let plural = if dmg > 1 { "s" } else { "" };
        format!("{calc} → {dmg} dégât{plural}{floor}, {}", Self::after(hp_after))
    }

    /// Like `hit_text`, when Senet turned `gain` of the damage into favors.
    fn senet_text(&self, raw: i32, def: i32, hp_after: i32, gain: i32) -> String {
        let dmg = self.damage(raw, def);
        let calc = if def > 0 { format!("{raw}⚔ − {def}🛡") } else { format!("{raw}⚔") };
        let plural = if dmg > 1 { "s" } else { "" };
        let part = if gain < dmg { format!("dont {gain} changé{} ", if gain > 1 { "s" } else { "" }) } else { format!("changé{plural} ") };
        let floor = if raw - def < dmg { " (minimum)" } else { "" };
        format!("{calc} → {dmg} dégât{plural}{floor}, {part}en {gain}🙏 (Senet), {}", Self::after(hp_after))
    }

    fn render(&self) -> String {
        let tok = |f: &F| {
            let d = &self.data.units[f.id];
            let star = if f.mult > 1 { "⭐" } else { "" };
            let mut s = format!("{star}{}{}/{}", d.emoji, f.atk, f.hp);
            if f.def > 0 {
                s += &format!("🛡{}", f.def);
            }
            s
        };
        let line = |s: usize| {
            let v: Vec<String> = self.sides[s].iter().map(tok).collect();
            format!("{} : {}", self.names[s], if v.is_empty() { "—".to_string() } else { v.join(" ") })
        };
        format!("{}\n{}", line(0), line(1))
    }

    /// Recounts every Phalanx bonus: once per ally sharing its ATK, once per ally sharing its
    /// PV, once per ally sharing its DEF (an ally matching two stats counts twice). Logs
    /// only the bonuses that changed.
    fn phalanx(&mut self) {
        for s in 0..2 {
            let mut updates = Vec::new();
            for (i, f) in self.sides[s].iter().enumerate() {
                if f.silenced && (f.ph_atk, f.ph_hp) != (0, 0) {
                    updates.push((i, 0, 0, [0, 0, 0]));
                }
                for k in self.kws(f) {
                    if let Keyword::Phalanx { atk, hp } = *k {
                        let me = f.core();
                        let mut same = [0, 0, 0];
                        for (_, o) in self.sides[s].iter().enumerate().filter(|&(j, _)| j != i) {
                            let o = o.core();
                            same[0] += (o.0 == me.0) as i32;
                            same[1] += (o.1 == me.1) as i32;
                            same[2] += (o.2 == me.2) as i32;
                        }
                        let times: i32 = same.iter().sum();
                        let (a, h) = (atk * f.mult * times, hp * f.mult * times);
                        if (a, h) != (f.ph_atk, f.ph_hp) {
                            updates.push((i, a, h, same));
                        }
                    }
                }
            }
            for (i, a, h, same) in updates {
                let n = self.label(s, &self.sides[s][i]);
                let f = &mut self.sides[s][i];
                let first = f.ph_atk == 0 && f.ph_hp == 0;
                f.atk = (f.atk - f.ph_atk + a).max(0);
                // Like a lost aura: max PV drops, current PV only down to the new max.
                let gain = h - f.ph_hp;
                f.max_hp += gain;
                f.hp = if gain > 0 { f.hp + gain } else { f.hp.min(f.max_hp) };
                (f.ph_atk, f.ph_hp) = (a, h);
                let why: Vec<String> = ["⚔", "❤", "🛡"]
                    .iter()
                    .zip(same)
                    .filter(|&(_, k)| k > 0)
                    .map(|(icon, k)| format!("{k}× même {icon}"))
                    .collect();
                let gains = crate::data::bonus(a, h, 0);
                if a == 0 && h == 0 {
                    say!(self.log, "    Phalange : {n} perd son bonus");
                } else if first {
                    say!(self.log, "    Phalange : {n} gagne {gains} ({})", why.join(", "));
                } else {
                    say!(self.log, "    Phalange : le bonus de {n} passe à {gains} ({})", why.join(", "));
                }
            }
        }
    }

    fn devour(&mut self, rng: &mut GameRng) {
        // Every Devour unit present at the start acts once, left to right, side A first. One
        // that has been eaten before its turn doesn't act.
        let eaters: Vec<(usize, usize)> = (0..2)
            .flat_map(|s| self.sides[s].iter().map(move |f| (s, f.slot)).collect::<Vec<_>>())
            .filter(|&(s, slot)| {
                let f = self.sides[s].iter().find(|f| f.slot == slot).unwrap();
                self.kws(f).contains(&Keyword::Devour)
            })
            .collect();
        for (s, slot) in eaters {
            let Some(i) = self.sides[s].iter().position(|f| f.slot == slot) else { continue };
            let targets: Vec<(usize, usize)> = (0..2)
                .flat_map(|ts| (0..self.sides[ts].len()).map(move |ti| (ts, ti)))
                .filter(|&(ts, ti)| !(ts == s && ti == i))
                .collect();
            let Some(&first) = targets.choose(rng) else { continue };
            // Fortune: landing on an ally counts as a failure, drawn again once.
            let retried = first.0 == s && self.perks[s].fortune;
            let (ts, ti) = if retried { *targets.choose(rng).unwrap() } else { first };
            let fail_note = if retried {
                format!("tombe sur {}, retente (Fortune) : ", self.label(first.0, &self.sides[first.0][first.1]))
            } else {
                String::new()
            };
            let prey = self.sides[ts].remove(ti);
            let i = if ts == s && ti < i { i - 1 } else { i };
            let f = &mut self.sides[s][i];
            let m = f.mult;
            f.atk += prey.atk * m;
            f.hp += prey.hp * m;
            f.max_hp += prey.hp * m;
            f.def += prey.def * m;
            if self.log.is_some() {
                let (n, pn) = (self.label(s, &self.sides[s][i]), self.label(ts, &prey));
                let gain = crate::data::bonus(prey.atk * m, prey.hp * m, prey.def * m);
                say!(self.log, "    Dévoration : {n} {fail_note}dévore {pn} : {gain}");
            }
        }
    }

    /// Bonne étoile: itself and its two neighbours gain the stats once per ⭐ ally on its board
    /// (itself included), for this fight. Every Bonne étoile counts the same board.
    fn lucky_star(&mut self) {
        for s in 0..2 {
            let stars = self.sides[s].iter().filter(|f| f.mult > 1).count() as i32;
            let mut gifts = Vec::new();
            for (i, f) in self.sides[s].iter().enumerate() {
                for k in self.kws(f) {
                    if let Keyword::LuckyStar { atk, hp } = *k {
                        gifts.push((i, atk * f.mult * stars, hp * f.mult * stars));
                    }
                }
            }
            for (i, a, h) in gifts {
                if stars == 0 {
                    let n = self.label(s, &self.sides[s][i]);
                    say!(self.log, "    Bonne étoile : {n} : aucune unité ⭐ sur le terrain");
                    continue;
                }
                let lo = i.saturating_sub(1);
                let hi = (i + 1).min(self.sides[s].len() - 1);
                let mut names = Vec::new();
                for j in lo..=hi {
                    let g = &mut self.sides[s][j];
                    g.atk += a;
                    g.hp += h;
                    g.max_hp += h;
                    if j != i {
                        names.push(self.label(s, &self.sides[s][j]));
                    }
                }
                if self.log.is_some() {
                    let n = self.label(s, &self.sides[s][i]);
                    let with = if names.is_empty() { String::new() } else { format!(" et {}", names.join(", ")) };
                    let plural = if stars > 1 { "s" } else { "" };
                    say!(self.log, "    Bonne étoile : {n}{with} : {} ({stars} unité{plural} ⭐ sur le terrain)", crate::data::bonus(a, h, 0));
                }
            }
        }
    }

    /// Each Petrify unit cancels the enemy's next attack (two for a ⭐ unit).
    fn petrify(&mut self) {
        for s in 0..2 {
            for i in 0..self.sides[s].len() {
                let f = &self.sides[s][i];
                if self.kws(f).contains(&Keyword::Petrify) {
                    let n = f.mult;
                    self.petrified[1 - s] += n;
                    if self.log.is_some() {
                        let what = if n > 1 { format!("ses {n} prochaines attaques") } else { "sa prochaine attaque".to_string() };
                        let caster = self.label(s, f);
                        say!(self.log, "    Pétrification : {caster} → {} : {what} annulée{}", self.names[1 - s], if n > 1 { "s" } else { "" });
                    }
                }
            }
        }
    }

    /// Emprise: every enemy loses ATK at the start of the fight.
    fn grip(&mut self) {
        let mut changed = false;
        for s in 0..2 {
            let amount = self.perks[s].grip;
            if amount <= 0 || self.sides[1 - s].is_empty() {
                continue;
            }
            for e in &mut self.sides[1 - s] {
                e.atk -= amount.min(e.atk.max(0));
            }
            changed = true;
            say!(self.log, "    Emprise : chaque unité de {} perd {amount}⚔", self.names[1 - s]);
        }
        if changed {
            self.phalanx();
        }
    }

    /// Coin flip with chance `p` for side `s`; with Fortune, a failure is flipped again once.
    /// Returns the outcome and whether Fortune was used.
    fn flip(&self, s: usize, p: f64, rng: &mut GameRng) -> (bool, bool) {
        if rng.gen_bool(p) {
            (true, false)
        } else if self.perks[s].fortune {
            (rng.gen_bool(p), true)
        } else {
            (false, false)
        }
    }

    /// Chill on attack: the enemy at the attacker's position (or the last enemy, when that
    /// line is shorter) loses `amount` ATK.
    fn chill(&mut self, s: usize, i: usize, amount: i32) {
        let enemies = self.sides[1 - s].len();
        if enemies == 0 {
            return;
        }
        let (ts, ti) = (1 - s, i.min(enemies - 1));
        let lost = amount.min(self.sides[ts][ti].atk.max(0));
        if lost > 0 {
            self.sides[ts][ti].atk -= lost;
            if self.log.is_some() {
                let (n, tn) = (self.label(s, &self.sides[s][i]), self.label(ts, &self.sides[ts][ti]));
                say!(self.log, "    Gel : {n} → {tn} perd {lost}⚔");
            }
        }
    }

    /// Hydromel: heals the living ally with the lowest PV among the wounded ones (the attacker
    /// included) by the damage dealt, up to its max PV.
    fn lifelink(&mut self, s: usize, attacker: &F, dealt: i32, rng: &mut GameRng) {
        let wounded: Vec<usize> = (0..self.sides[s].len())
            .filter(|&j| {
                let f = &self.sides[s][j];
                f.hp > 0 && f.hp < f.max_hp
            })
            .collect();
        let Some(low) = wounded.iter().map(|&j| self.sides[s][j].hp).min() else { return };
        let lowest: Vec<usize> = wounded.into_iter().filter(|&j| self.sides[s][j].hp == low).collect();
        let j = *lowest.choose(rng).unwrap();
        let f = &mut self.sides[s][j];
        let healed = dealt.min(f.max_hp - f.hp);
        f.hp += healed;
        if self.log.is_some() {
            let (an, tn) = (self.label(s, attacker), self.label(s, &self.sides[s][j]));
            let hp = self.sides[s][j].hp;
            say!(self.log, "    Hydromel : {an} soigne {tn} de {healed}❤, reste {hp}❤");
        }
    }

    /// Chaos on attack: `dmg` to a random living unit of either side (not itself), ignoring DEF.
    fn chaos(&mut self, s: usize, attacker: &F, dmg: i32, rng: &mut GameRng) {
        let mut targets = Vec::new();
        for ts in 0..2 {
            for (ti, f) in self.sides[ts].iter().enumerate() {
                if f.hp > 0 && !(ts == s && f.slot == attacker.slot) {
                    targets.push((ts, ti));
                }
            }
        }
        let Some(&first) = targets.choose(rng) else { return };
        let retry = first.0 == s && self.perks[s].fortune;
        let (ts, ti) = if retry { *targets.choose(rng).unwrap() } else { first };
        let note = if retry && self.log.is_some() {
            format!("tombe sur {}, retente (Fortune) → ", self.label(first.0, &self.sides[first.0][first.1]))
        } else {
            String::new()
        };
        self.sides[ts][ti].hp -= dmg;
        if self.log.is_some() {
            let (an, tn) = (self.label(s, attacker), self.label(ts, &self.sides[ts][ti]));
            let plural = if dmg > 1 { "s" } else { "" };
            let after = Self::after(self.sides[ts][ti].hp);
            say!(self.log, "    Chaos : {an} → {note}{tn} : {dmg} dégât{plural} (ignore la DÉF), {after}");
        }
    }

    /// Raise on attack: 50 % chance (75 % for a ⭐ unit) to bring back a random dead ally with
    /// 1 PV, at its original place in the line.
    /// Momification never brings back another Embaumeur (or any Raise unit), so two of them
    /// can't keep reviving each other.
    fn raise(&mut self, s: usize, caster: &F, rng: &mut GameRng) {
        let eligible: Vec<usize> = (0..self.graveyard[s].len())
            .filter(|&k| !self.kws(&self.graveyard[s][k]).contains(&Keyword::Raise))
            .collect();
        if eligible.is_empty() {
            return;
        }
        let chance = if caster.mult > 1 { 0.75 } else { 0.5 };
        let cn = self.label(s, caster);
        let (ok, retried) = self.flip(s, chance, rng);
        let fortune = if retried { " (Fortune : retenté)" } else { "" };
        if !ok {
            say!(self.log, "    Momification : {cn} échoue{fortune}");
            return;
        }
        let k = *eligible.choose(rng).unwrap();
        let mut back = self.graveyard[s].remove(k);
        back.hp = 1;
        back.attacked = true;
        let at = self.sides[s].iter().position(|f| f.slot > back.slot).unwrap_or(self.sides[s].len());
        let n = self.label(s, &back);
        self.sides[s].insert(at, back);
        say!(self.log, "    Momification : {cn} ramène {n} avec 1❤{fortune}");
    }

    /// Résurrection: the unit at `i` comes back with 1 PV, with every ally already dead in this
    /// fight (at its original place, waiting for the next pass to attack) and every ally falling
    /// at the same time.
    fn resurrect(&mut self, s: usize, i: usize) {
        let caster = &mut self.sides[s][i];
        caster.hp = 1;
        caster.rebirths_left -= 1;
        let slot = caster.slot;
        self.rebirths[s][slot] += 1;
        let mut back = Vec::new();
        for j in 0..self.sides[s].len() {
            if j != i && self.sides[s][j].hp <= 0 && !self.sides[s][j].destroyed {
                self.sides[s][j].hp = 1;
                back.push(self.label(s, &self.sides[s][j]));
            }
        }
        for mut f in std::mem::take(&mut self.graveyard[s]) {
            f.hp = 1;
            f.attacked = true;
            back.push(self.label(s, &f));
            let at = self.sides[s].iter().position(|g| g.slot > f.slot).unwrap_or(self.sides[s].len());
            self.sides[s].insert(at, f);
        }
        if self.log.is_some() {
            let n = self.label(s, self.sides[s].iter().find(|f| f.slot == slot).unwrap());
            let others = if back.is_empty() { String::new() } else { format!(" et ramène {}", back.join(", ")) };
            say!(self.log, "    Résurrection : {n} renaît{others} avec 1❤");
        }
    }

    /// Héritage: the ally standing at `next` (right of the dead unit) gets its ATK.
    fn legacy(&mut self, s: usize, dead: &F, next: usize) {
        if !self.kws(dead).contains(&Keyword::Legacy) {
            return;
        }
        if let Some(n) = self.sides[s].get(next) {
            let next_name = self.label(s, n);
            let gift = dead.atk.max(0) * dead.mult;
            if gift == 0 {
                return;
            }
            self.sides[s][next].atk += gift;
            say!(self.log, "    Héritage : {next_name} reçoit +{gift}⚔");
        }
    }

    /// Éternel retour returns the side still has: the synergy's one, plus those of its units
    /// with the power (on the field or dead in this fight; ⭐ ones count double).
    fn returns_left(&self, s: usize) -> i32 {
        let units: i32 = self.sides[s]
            .iter()
            .chain(&self.graveyard[s])
            .filter(|f| self.kws(f).contains(&Keyword::Recurrence))
            .map(|f| f.mult)
            .sum();
        self.perks[s].recurrence as i32 + units - self.recurred[s]
    }

    fn resolve_deaths(&mut self, s: usize) {
        let mut i = 0;
        while i < self.sides[s].len() {
            if self.sides[s][i].hp > 0 {
                i += 1;
                continue;
            }
            let mut dead = self.sides[s][i].clone();
            let n = self.label(s, &dead);
            // Néant: gone for good, nothing triggers.
            if dead.destroyed {
                self.sides[s].remove(i);
                self.destroyed[s][dead.slot] = true;
                continue;
            }
            let kws = self.kws(&dead);
            if dead.rebirths_left > 0 && kws.contains(&Keyword::Resurrect) {
                self.legacy(s, &dead, i + 1);
                self.resurrect(s, i);
                // Units came back on both sides of i: check the whole line again.
                i = 0;
                continue;
            }
            if dead.rebirths_left > 0 && kws.contains(&Keyword::Rebirth) {
                self.legacy(s, &dead, i + 1);
                let f = &mut self.sides[s][i];
                f.hp = 1;
                f.rebirths_left -= 1;
                self.rebirths[s][dead.slot] += 1;
                say!(self.log, "    {n} renaît");
                i += 1;
                continue;
            }
            if self.returns_left(s) > 0 {
                self.legacy(s, &dead, i + 1);
                self.recurred[s] += 1;
                self.sides[s][i].hp = 1;
                say!(self.log, "    Éternel retour : {n} revient avec 1❤");
                i += 1;
                continue;
            }
            self.sides[s].remove(i);
            for k in kws {
                if let Keyword::Offering { deaths, essence } = *k {
                    if dead.quest_done {
                        continue;
                    }
                    let step = dead.mult as u32;
                    dead.deaths += step;
                    self.deaths[s][dead.slot] += step;
                    let done = dead.deaths.min(deaths);
                    if dead.deaths >= deaths {
                        dead.quest_done = true;
                        self.essence[s] += essence;
                        say!(self.log, "    Offrande : {n} accomplit sa quête ({done}/{deaths} morts) et soigne {essence}✨ à {}", self.names[s]);
                    } else {
                        say!(self.log, "    Offrande : {n} : {done}/{deaths} morts");
                    }
                }
            }
            self.graveyard[s].push(dead.clone());
            // The unit that was behind the dead one now stands at index i.
            self.legacy(s, &dead, i);
        }
    }

    fn combat(&mut self, rng: &mut GameRng) -> Outcome {
        let (na, nb) = (self.sides[0].len(), self.sides[1].len());
        let mut turn = if na != nb { (nb > na) as usize } else { rng.gen_bool(0.5) as usize };
        if self.log.is_some() && na > 0 && nb > 0 {
            let why = if na != nb { "plus d'unités" } else { "tirage au sort" };
            say!(self.log, "    {} attaque en premier ({why})", self.names[turn]);
        }
        for _ in 0..self.data.rules.max_fight_steps {
            if self.sides[0].is_empty() || self.sides[1].is_empty() {
                break;
            }
            let s = turn;
            turn = 1 - s;
            // The leftmost unit that hasn't attacked yet; once all have, start again.
            let i = match self.sides[s].iter().position(|f| !f.attacked) {
                Some(i) => i,
                None => {
                    self.sides[s].iter_mut().for_each(|f| f.attacked = false);
                    0
                }
            };
            self.sides[s][i].attacked = true;
            if self.petrified[s] > 0 {
                self.petrified[s] -= 1;
                let n = self.label(s, &self.sides[s][i]);
                say!(self.log, "    {n} est pétrifié : attaque annulée");
                continue;
            }
            let kws = self.kws(&self.sides[s][i]);
            for k in kws {
                if let Keyword::Chill { amount } = *k {
                    self.chill(s, i, amount * self.sides[s][i].mult);
                }
            }
            self.phalanx();
            // The attacker and its random target strike each other at the same time.
            // Provocation: the target must be one of the taunting enemies, if any.
            let taunters: Vec<usize> = (0..self.sides[1 - s].len())
                .filter(|&j| self.kws(&self.sides[1 - s][j]).contains(&Keyword::Taunt))
                .collect();
            let t = match taunters.choose(rng) {
                Some(&j) => j,
                // A u64 range draws the same numbers on 32-bit targets (WebAssembly), where a
                // usize range would consume a u32 instead.
                None => rng.gen_range(0..self.sides[1 - s].len() as u64) as usize,
            };
            let taunted = if taunters.is_empty() { "" } else { " (provocation)" };
            let (att, mut tgt) = (self.sides[s][i].clone(), self.sides[1 - s][t].clone());
            let mut power = att.atk;
            if kws.contains(&Keyword::Hubris) {
                let (heads, retried) = self.flip(s, 0.5, rng);
                if heads {
                    power *= 2;
                }
                if self.log.is_some() {
                    let an = self.label(s, &att);
                    let what = match (heads, retried) {
                        (true, false) => "pile, ATQ doublée pour ce coup",
                        (true, true) => "face, retenté (Fortune) : pile, ATQ doublée pour ce coup",
                        (false, true) => "face, retenté (Fortune) : face",
                        (false, false) => "face",
                    };
                    say!(self.log, "    Démesure : {an} : {what}");
                }
            }
            let mut dealt = self.damage(power, tgt.def);
            // Senet: heads, the damage becomes favors, as long as the fight's cap isn't reached.
            let mut gain = 0;
            let senet = kws.iter().find_map(|k| match *k {
                Keyword::Senet { max } => Some(max * att.mult),
                _ => None,
            });
            if let Some(cap) = senet.filter(|&cap| self.favors[s] < cap) {
                let (heads, retried) = self.flip(s, 0.5, rng);
                if heads {
                    gain = dealt.min(cap - self.favors[s]);
                    self.favors[s] += gain;
                    dealt -= gain;
                }
                if self.log.is_some() {
                    let an = self.label(s, &att);
                    let what = match (heads, retried) {
                        (true, false) => "pile",
                        (true, true) => "face, retenté (Fortune) : pile",
                        (false, true) => "face, retenté (Fortune) : face",
                        (false, false) => "face",
                    };
                    say!(self.log, "    Senet : {an} : {what}");
                }
            }
            self.sides[1 - s][t].hp -= dealt;
            self.sides[s][i].hp -= self.damage(tgt.atk, att.def);
            if self.log.is_some() {
                let (an, tn) = (self.label(s, &att), self.label(1 - s, &tgt));
                let hp = self.sides[1 - s][t].hp;
                let hit = if gain > 0 { self.senet_text(power, tgt.def, hp, gain) } else { self.hit_text(power, tgt.def, hp) };
                let back = self.hit_text(tgt.atk, att.def, self.sides[s][i].hp);
                say!(self.log, "    {an} attaque {tn}{taunted} : {hit}");
                say!(self.log, "    {tn} riposte : {back}");
            }
            // Regard de pierre lands with the blow: the target's powers are gone, Représailles,
            // Néant and Châtiment included.
            if kws.contains(&Keyword::Silence) && !tgt.silenced && !self.data.units[tgt.id].keywords.is_empty() {
                self.sides[1 - s][t].silenced = true;
                tgt.silenced = true;
                let (an, tn) = (self.label(s, &att), self.label(1 - s, &tgt));
                say!(self.log, "    Regard de pierre : {an} → {tn} perd tous ses pouvoirs");
            }
            // Units killed by the blows: Néant destroys them, Châtiment kills their killer.
            let tgt_fell = self.sides[1 - s][t].hp <= 0 && tgt.hp > 0;
            let att_fell = self.sides[s][i].hp <= 0 && att.hp > 0;
            for (ks, ki, killer, vs, vi, fell) in [(s, i, &att, 1 - s, t, tgt_fell), (1 - s, t, &tgt, s, i, att_fell)] {
                if !fell {
                    continue;
                }
                let kk = self.kws(killer);
                if kk.contains(&Keyword::Annihilate) && !self.sides[vs][vi].destroyed {
                    self.sides[vs][vi].destroyed = true;
                    let (kn, vn) = (self.label(ks, killer), self.label(vs, &self.sides[vs][vi]));
                    say!(self.log, "    Néant : {kn} anéantit {vn}");
                }
                if kk.contains(&Keyword::Vengeance) && self.sides[ks][ki].hp > 0 {
                    self.sides[ks][ki].hp = 0;
                    let kn = self.label(ks, killer);
                    say!(self.log, "    Châtiment : {kn} a tué, sa vengeance est accomplie : mort");
                }
            }
            // Thorns: the target hits back again, ignoring DEF, even if it fell.
            for k in self.kws(&tgt) {
                if let Keyword::Thorns { dmg } = *k {
                    let dmg = dmg * tgt.mult;
                    self.sides[s][i].hp -= dmg;
                    if self.log.is_some() {
                        let (an, tn) = (self.label(s, &att), self.label(1 - s, &tgt));
                        let plural = if dmg > 1 { "s" } else { "" };
                        let left = Self::after(self.sides[s][i].hp);
                        say!(self.log, "    Représailles : {tn} → {an} : {dmg} dégât{plural} (ignore la DÉF), {left}");
                    }
                }
                // Effroi: the attacker loses ATK for the rest of the fight.
                if let Keyword::Dread { atk } = *k {
                    let lost = (atk * tgt.mult).min(self.sides[s][i].atk.max(0));
                    if lost > 0 {
                        self.sides[s][i].atk -= lost;
                        let (an, tn) = (self.label(s, &att), self.label(1 - s, &tgt));
                        say!(self.log, "    Effroi : {tn} → {an} perd {lost}⚔");
                    }
                }
            }
            // On-attack effects.
            for k in kws {
                match *k {
                    Keyword::Steal { amount } => {
                        let taken = (amount * att.mult).min(self.sides[1 - s][t].atk.max(0));
                        if taken > 0 {
                            self.sides[1 - s][t].atk -= taken;
                            self.sides[s][i].atk += taken;
                            let (an, tn) = (self.label(s, &att), self.label(1 - s, &tgt));
                            say!(self.log, "    Fascination : {an} prend {taken}⚔ à {tn}");
                        }
                    }
                    Keyword::Chaos { dmg } => self.chaos(s, &att, dmg * att.mult, rng),
                    Keyword::Lifelink => self.lifelink(s, &att, dealt, rng),
                    _ => {}
                }
            }
            // Last, as it can shift positions on the attacker's side.
            if kws.contains(&Keyword::Raise) {
                self.raise(s, &att, rng);
            }
            self.resolve_deaths(0);
            self.resolve_deaths(1);
            self.phalanx();
        }
        match (self.sides[0].is_empty(), self.sides[1].is_empty()) {
            (false, true) => Outcome::WinA,
            (true, false) => Outcome::WinB,
            _ => Outcome::Draw,
        }
    }
}
