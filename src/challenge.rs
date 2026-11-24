//! Arena challenges (regles.md §7): the challenger's composition and three dice, the defender's
//! composition and three dice. The six dice seed the fight, so anyone can replay it. This module
//! plays it, settles the result (🏆, ✨, series, Rancune, Offrande…) and writes the forum post.

use crate::compo::{self, Composition, Parsed};
use crate::data::{Data, Keyword};
use crate::fight::{self, FightResult, Outcome};
use crate::forum;
use crate::game;
use crate::state::{self, GameRng, Unit};
use rand::SeedableRng;

/// Dice of a challenge: three from the challenger, then three from the defender.
pub const DICE: usize = 6;

/// The fight seed: Σ (dᵢ − 1) · 100^(5 − i), the challenger's dice first. It stays below 10¹²,
/// so it is exact in a JavaScript number too.
pub fn seed(dice: &[u32]) -> Result<u64, String> {
    if dice.len() != DICE {
        return Err(format!("Il faut {DICE} dés (3 de l'attaquant puis 3 du défenseur), pas {}", dice.len()));
    }
    let mut s: u64 = 0;
    for (k, &d) in dice.iter().enumerate() {
        if !(1..=100).contains(&d) {
            return Err(format!("Dé n°{} : {d} n'est pas entre 1 et 100", k + 1));
        }
        s = s * 100 + (d as u64 - 1);
    }
    Ok(s)
}

/// "2 défaites d'affilée", noting the cap of Déchaînement when the streak is longer.
fn streak_text(streak: i32, max: i32) -> String {
    let s = if streak > 1 { "s" } else { "" };
    if streak > max {
        format!("{streak} défaite{s} d'affilée, comptée{s} {max}")
    } else {
        format!("{streak} défaite{s} d'affilée")
    }
}

fn wins_text(wins: i32) -> String {
    format!("{wins} victoire{} d'affilée", if wins > 1 { "s" } else { "" })
}

/// What a fight changes for both players.
#[derive(Debug, Clone)]
pub struct Settlement {
    /// Side of the winner (0 = challenger), None on a draw.
    pub winner: Option<usize>,
    /// The winner had fewer 🏆 than the loser.
    pub upset: bool,
    /// Bilan lines, for the forum.
    pub lines: Vec<String>,
    /// Both compositions after the fight (bonuses, quests, 🏆, ✨, series).
    pub after: [Composition; 2],
    /// Players who fell to 0✨ and start over.
    pub reset: [bool; 2],
}

/// A played challenge.
pub struct Challenge {
    pub names: [String; 2],
    pub compos: [Composition; 2],
    pub dice: Vec<u32>,
    pub seed: u64,
    /// Déchaînement and synergy lines shown under the line-ups.
    pub prelude: Vec<String>,
    /// The traced fight.
    pub log: Vec<String>,
    pub result: FightResult,
    pub settlement: Settlement,
    /// Warnings from both compositions, with the player's name.
    pub warnings: Vec<String>,
    pub data_hash: String,
}

/// Reads both compositions and plays the challenge. `names` override the names written in the
/// compositions. Errors are in French, prefixed with the side they come from.
pub fn challenge(data: &Data, texts: [&str; 2], names: [Option<&str>; 2], dice: &[u32]) -> Result<Challenge, Vec<String>> {
    let parsed: [Parsed; 2] = texts.map(|t| compo::parse(data, t));
    let roles = ["Attaquant", "Défenseur"];
    let names: [String; 2] = [0, 1].map(|s| {
        let given = names[s].map(str::trim).filter(|n| !n.is_empty());
        let written = Some(parsed[s].compo.name.trim()).filter(|n| !n.is_empty());
        given.or(written).unwrap_or(roles[s]).to_string()
    });
    let who = |s: usize| if names[s] == roles[s] { roles[s].to_string() } else { format!("{} ({})", roles[s], names[s]) };
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (s, p) in parsed.iter().enumerate() {
        errors.extend(p.errors.iter().map(|e| format!("{} : {e}", who(s))));
        warnings.extend(p.warnings.iter().map(|w| format!("{} : {w}", who(s))));
    }
    let seed = seed(dice).unwrap_or_else(|e| {
        errors.push(e);
        0
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    if names[0] == names[1] {
        warnings.push(format!("Les deux joueurs s'appellent {} : le déroulé sera difficile à lire", names[0]));
    }
    let compos: [Composition; 2] = [0, 1].map(|s| Composition { name: names[s].clone(), ..parsed[s].compo.clone() });
    let mut c = play(data, names, compos, dice.to_vec(), seed);
    c.warnings = warnings;
    Ok(c)
}

/// Plays a challenge between two valid compositions.
pub fn play(data: &Data, names: [String; 2], compos: [Composition; 2], dice: Vec<u32>, seed: u64) -> Challenge {
    let max = data.rules.arena.unleash_max;
    let boards: [Vec<Unit>; 2] = [compos[0].board(data), compos[1].board(data)];
    let mut fighting = Vec::new();
    let mut prelude = Vec::new();
    for s in 0..2 {
        let streak = compos[s].streak.unwrap_or(0).max(0);
        let (board, line) = game::unleash(data, &boards[s], streak.min(max), &names[s], &streak_text(streak, max));
        fighting.push(board);
        prelude.extend(line);
    }
    for s in 0..2 {
        prelude.extend(game::synergy_line(data, &names[s], &boards[s]));
    }
    let perks = [game::board_perks(data, &boards[0]), game::board_perks(data, &boards[1])];
    let mut rng = GameRng::seed_from_u64(seed);
    let mut log = Vec::new();
    let result = fight::fight(data, [&names[0], &names[1]], &fighting[0], &fighting[1], perks, &mut rng, Some(&mut log));
    let settlement = settle(data, &names, &compos, &boards, &result);
    Challenge {
        names,
        compos,
        dice,
        seed,
        prelude,
        log,
        result,
        settlement,
        warnings: Vec::new(),
        data_hash: data.hash_label(),
    }
}

/// "🐗 Sanglier", with its ⭐.
fn unit_name(data: &Data, u: &Unit) -> String {
    let d = &data.units[u.id];
    format!("{}{} {}", if u.merged { "⭐" } else { "" }, d.emoji, d.name)
}

/// Applies a fight result as `arena::fight` does. `boards` are the boards as written (before
/// Déchaînement).
pub fn settle(data: &Data, names: &[String; 2], compos: &[Composition; 2], boards: &[Vec<Unit>; 2], res: &FightResult) -> Settlement {
    let r = &data.rules;
    let a = &r.arena;
    let max_essence = r.starting_essence;
    let mut lines = Vec::new();
    let mut after_boards = boards.clone();
    let mut vp = [0, 1].map(|s| compos[s].vp.unwrap_or(0));
    let mut essence = [0, 1].map(|s| compos[s].essence);
    let mut streak = [0, 1].map(|s| compos[s].streak.unwrap_or(0).max(0));
    let mut wins = [0, 1].map(|s| compos[s].wins.unwrap_or(0).max(0));
    let mut reset = [false, false];

    // Both sides: Offering progress and heal, Senet.
    for s in 0..2 {
        state::record_deaths(data, &mut after_boards[s], &res.deaths[s]);
        for (k, u) in after_boards[s].iter().enumerate() {
            if res.deaths[s].get(k).copied().unwrap_or(0) == 0 {
                continue;
            }
            if let Some(n) = compo::offering_needed(data, u.id) {
                let done = if u.deaths >= n { " : quête accomplie" } else { "" };
                lines.push(format!("Offrande : {} ({}) en est à {}/{n} morts{done}", unit_name(data, u), names[s], u.deaths.min(n)));
            }
        }
        let healed = res.essence[s];
        if healed > 0 {
            match essence[s] {
                Some(e) => {
                    let now = (e + healed).min(max_essence.max(e));
                    let capped = if now < e + healed { format!(", plafonné à {max_essence}✨") } else { String::new() };
                    lines.push(format!("{} récupère {healed}✨ grâce à l'Offrande{capped} : {now}✨", names[s]));
                    essence[s] = Some(now);
                }
                None => lines.push(format!("{} récupère {healed}✨ grâce à l'Offrande ({max_essence}✨ au plus)", names[s])),
            }
        }
        if res.favors[s] > 0 {
            let cap = boards[s]
                .iter()
                .filter_map(|u| data.units[u.id].keywords.iter().find_map(|k| match *k {
                    Keyword::Senet { max } => Some(max * u.mult()),
                    _ => None,
                }))
                .max()
                .unwrap_or(0);
            lines.push(format!(
                "Senet : {} gagne {}🙏, dans la limite de {cap}🙏 de Senet par jour",
                names[s], res.favors[s]
            ));
        }
    }

    // Néant: destroyed units leave their owner's composition for good.
    for s in 0..2 {
        let mut gone = Vec::new();
        for k in (0..after_boards[s].len()).rev() {
            if res.destroyed[s].get(k).copied().unwrap_or(false) {
                let u = after_boards[s].remove(k);
                let back = if u.merged { format!("ses {} exemplaires retournent", r.merge_copies) } else { "retourne".to_string() };
                gone.push(format!(
                    "Néant : {} ({}) est anéanti : il quitte sa composition et {back} dans la réserve commune",
                    unit_name(data, &u),
                    names[s]
                ));
            }
        }
        lines.extend(gone.into_iter().rev());
    }

    let winner = match res.outcome {
        Outcome::WinA => Some(0),
        Outcome::WinB => Some(1),
        Outcome::Draw => None,
    };
    let mut upset = false;
    match winner {
        Some(w) => {
            let l = 1 - w;
            upset = vp[w] < vp[l];
            let gain = 1 + upset as i32;
            if upset {
                lines.push(format!(
                    "Exploit : {} avait moins de 🏆 que {} ({} contre {}) : +2🏆, et la perte {} est doublée",
                    names[w], names[l], vp[w], vp[l], forum::de(&names[l])
                ));
            }
            wins[w] += 1;
            // Streak bonus: from the Nth win in a row, each win pays more.
            let bonus = if wins[w] >= a.win_streak_from as i32 { a.win_streak_favors } else { 0 };
            let bonus_text = if bonus > 0 { format!(" (dont +{bonus}🙏 de bonus de série)") } else { String::new() };
            let reset_streak = if streak[w] > 0 { " · défaites d'affilée remises à 0" } else { "" };
            lines.push(format!(
                "{} : +{gain}🏆 ({} → {}🏆) · +{}🙏{bonus_text} · 🔺 {}{reset_streak}",
                names[w],
                vp[w],
                vp[w] + gain,
                a.win_favors + bonus,
                wins_text(wins[w])
            ));
            vp[w] += gain;
            streak[w] = 0;

            // The loser: essence lost for every unit of the winner still standing.
            let standing: Vec<String> = res.survivors[w]
                .iter()
                .map(|&(id, mult)| {
                    let d = &data.units[id];
                    let star = if mult > 1 { "⭐" } else { "" };
                    let cost = d.tier as i32 * r.loss_per_tier * if mult > 1 { r.merged_factor } else { 1 };
                    format!("{star}{} {cost}", d.emoji)
                })
                .collect();
            let base = data.defeat_loss(&res.survivors[w]);
            lines.push(format!("Survivants {} : {} = {base}✨", forum::de(&names[w]), standing.join(" + ")));
            let raw = base * (1 + upset as i32);
            let endure = state::best_endure(data, &boards[l]);
            let lost = state::endured(endure, raw);
            let mut how = String::new();
            if upset {
                how += " ×2 exploit";
            }
            if lost != raw {
                how += &format!(" − {} Endurance", raw - lost);
            }
            let how = if how.is_empty() { how } else { format!(" ({base}{how})") };
            streak[l] += 1;
            let broken = if wins[l] > 0 { format!(" · fin de sa série de {}", wins_text(wins[l])) } else { String::new() };
            wins[l] = 0;
            let left = match essence[l] {
                Some(e) => {
                    essence[l] = Some(e - lost);
                    format!(" · reste {}✨", (e - lost).max(0))
                }
                None => String::new(),
            };
            lines.push(format!("{} perd {lost}✨{how}{left} · 🔻 {}{broken}", names[l], streak_text(streak[l], i32::MAX)));

            // A loser who falls to 0✨ loses its units: nothing more to apply.
            if !essence[l].is_some_and(|e| e <= 0) {
                // Rancune: permanent stats.
                let before = after_boards[l].clone();
                state::apply_grudge(data, &mut after_boards[l]);
                for (old, new) in before.iter().zip(&after_boards[l]) {
                    if (old.atk, old.hp, old.def) != (new.atk, new.hp, new.def) {
                        lines.push(format!(
                            "Rancune : {} ({}) {} en bonus permanent : {}/{}/{}",
                            unit_name(data, new),
                            names[l],
                            crate::data::bonus(new.atk - old.atk, new.hp - old.hp, new.def - old.def),
                            new.atk,
                            new.hp,
                            new.def
                        ));
                    }
                }
                // Pillage: paid at the next daily tick, per defeat of the day.
                let plunder = boards[l]
                    .iter()
                    .filter(|u| data.units[u.id].has(|k| matches!(k, Keyword::Plunder { .. })))
                    .max_by_key(|u| u.mult());
                if let Some(u) = plunder {
                    lines.push(format!(
                        "Pillage : {} ({}) rapportera +{}🙏 pour cette défaite (versé chaque jour)",
                        unit_name(data, u),
                        names[l],
                        a.plunder_per_loss * u.mult()
                    ));
                }
                // Comeback prime: every N defeats in a row.
                if a.comeback_losses > 0 && streak[l] % a.comeback_losses as i32 == 0 {
                    lines.push(format!(
                        "Prime de retour : {} pour {} : +{}🙏",
                        streak_text(streak[l], i32::MAX),
                        names[l],
                        a.comeback_favors
                    ));
                }
            }
        }
        None => {
            for s in 0..2 {
                let left = match essence[s] {
                    Some(e) => {
                        essence[s] = Some(e - a.draw_loss);
                        format!(" · reste {}✨", (e - a.draw_loss).max(0))
                    }
                    None => String::new(),
                };
                lines.push(format!("Match nul : {} perd {}✨{left}", names[s], a.draw_loss));
            }
        }
    }

    // At 0✨ a player starts over: everything is lost, 🏆 included.
    for s in 0..2 {
        if essence[s].is_some_and(|e| e <= 0) {
            reset[s] = true;
            lines.push(format!(
                "💀 {} tombe à 0✨ : terrain, réserve, faveurs, quêtes et {}🏆 perdus ; il repart de zéro avec {max_essence}✨",
                names[s], vp[s]
            ));
        }
    }

    let after = [0, 1].map(|s| {
        if reset[s] {
            Composition { name: names[s].clone(), vp: Some(0), essence: Some(max_essence), streak: Some(0), wins: Some(0), units: Vec::new() }
        } else {
            Composition {
                name: names[s].clone(),
                vp: Some(vp[s]),
                essence: essence[s],
                streak: Some(streak[s]),
                wins: Some(wins[s]),
                units: Composition::entries(data, &after_boards[s]),
            }
        }
    });
    Settlement { winner, upset, lines, after, reset }
}

impl Challenge {
    /// "Victoire de Alice (exploit)", "Match nul".
    pub fn verdict(&self) -> String {
        match self.settlement.winner {
            Some(w) => format!("Victoire {}{}", forum::de(&self.names[w]), if self.settlement.upset { " (exploit)" } else { "" }),
            None => "Match nul".to_string(),
        }
    }

    /// "Seed 110276440899 · dés 12 3 77 / 45 9 100 · données a1b2c3d4 · moteur 0.1.0"
    pub fn footer(&self) -> String {
        let dice = |r: &[u32]| r.iter().map(u32::to_string).collect::<Vec<_>>().join(" ");
        format!(
            "Seed {} · dés {} / {} · données {} · moteur {}",
            self.seed,
            dice(&self.dice[..3]),
            dice(&self.dice[3..]),
            self.data_hash,
            crate::ENGINE_VERSION
        )
    }

    /// The post for the forum (Twino).
    pub fn forum_text(&self) -> String {
        let title = format!("⚔ Défi : {} contre {}", self.names[0], self.names[1]);
        let r = &self.result;
        let mut lines = forum::fight_post(&title, &r.start, &self.prelude, &self.log, &self.verdict(), &r.end);
        let hr = lines.pop();
        lines.push(String::new());
        lines.push("[b]Bilan[/b]".to_string());
        lines.push("[spoiler]".to_string());
        lines.extend(self.settlement.lines.iter().cloned());
        lines.push("[/spoiler]".to_string());
        lines.extend(hr);
        lines.push(format!("[i]{}[/i]", self.footer()));
        lines.join("\n")
    }

    /// The same post without markup.
    pub fn plain_text(&self) -> String {
        forum::plain(&self.forum_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compo::Entry;

    fn data() -> Data {
        Data::from_strs(include_str!("../data/units.toml"), include_str!("../data/rules.toml")).unwrap()
    }

    #[test]
    fn seeds() {
        assert_eq!(seed(&[12, 3, 77, 45, 9, 100]), Ok(110_276_440_899));
        assert_eq!(seed(&[1; 6]), Ok(0));
        assert_eq!(seed(&[100; 6]), Ok(999_999_999_999));
        assert!(seed(&[0, 1, 1, 1, 1, 1]).is_err());
        assert!(seed(&[1, 1, 1, 1, 1, 101]).is_err());
        assert!(seed(&[1, 1, 1]).is_err());
    }

    fn compo(data: &Data, units: &[(&str, bool, i32, i32, i32, u32)], vp: i32, essence: i32, streak: i32) -> Composition {
        let units = units
            .iter()
            .map(|&(id, star, atk, hp, def, deaths)| {
                assert!(data.unit_index(id).is_some(), "{id}");
                Entry { id: id.to_string(), star, atk, hp, def, deaths }
            })
            .collect();
        Composition { name: String::new(), vp: Some(vp), essence: Some(essence), streak: Some(streak), wins: Some(0), units }
    }

    fn result(outcome: Outcome, survivors: [Vec<(usize, i32)>; 2], sizes: [usize; 2]) -> FightResult {
        FightResult {
            outcome,
            rebirths: sizes.map(|n| vec![0; n]),
            essence: [0, 0],
            destroyed: sizes.map(|n| vec![false; n]),
            favors: [0, 0],
            deaths: sizes.map(|n| vec![0; n]),
            survivors,
            start: String::new(),
            end: String::new(),
        }
    }

    fn names() -> [String; 2] {
        ["Alice".to_string(), "Bob".to_string()]
    }

    #[test]
    fn upset_with_endurance() {
        let data = data();
        let vouivre = data.unit_index("vouivre").unwrap();
        // Bob: three Revanche units (Endurance), Alice has fewer 🏆.
        let a = compo(&data, &[("vouivre", false, 6, 7, 2, 0)], 1, 20, 2);
        let b = compo(
            &data,
            &[("sisyphus", false, 0, 3, 0, 0), ("hydra", false, 2, 3, 0, 0), ("tomb_robber", false, 3, 3, 0, 0)],
            4,
            10,
            1,
        );
        let compos = [a, b];
        let boards = [compos[0].board(&data), compos[1].board(&data)];
        // The Vouivre (rank 3) survives: 3✨, doubled by the upset = 6, −2 Endurance = 4.
        let res = result(Outcome::WinA, [vec![(vouivre, 1)], vec![]], [1, 3]);
        let s = settle(&data, &names(), &compos, &boards, &res);
        assert_eq!(s.winner, Some(0));
        assert!(s.upset);
        assert_eq!(s.after[0].vp, Some(3));
        assert_eq!(s.after[0].streak, Some(0));
        assert_eq!(s.after[1].essence, Some(6));
        assert_eq!(s.after[1].streak, Some(2));
        assert_eq!(s.after[1].vp, Some(4));
        // Rancune: Sisyphe and the Hydra keep +1⚔ +1❤.
        assert_eq!((s.after[1].units[0].atk, s.after[1].units[0].hp), (1, 4));
        assert_eq!((s.after[1].units[1].atk, s.after[1].units[1].hp), (3, 4));
        assert!(s.lines.iter().any(|l| l.contains("Pillage")));
    }

    #[test]
    fn streak_bonus_and_comeback_prime() {
        let data = data();
        let vouivre = data.unit_index("vouivre").unwrap();
        // Alice wins her 3rd in a row (+1🙏 bonus); Bob loses his 3rd in a row (prime +5🙏).
        let mut a = compo(&data, &[("vouivre", false, 6, 7, 2, 0)], 4, 20, 0);
        a.wins = Some(2);
        let mut b = compo(&data, &[("boar", false, 1, 4, 0, 0)], 1, 20, 2);
        b.wins = Some(1);
        let compos = [a, b];
        let boards = [compos[0].board(&data), compos[1].board(&data)];
        let res = result(Outcome::WinA, [vec![(vouivre, 1)], vec![]], [0, 1]);
        let s = settle(&data, &names(), &compos, &boards, &res);
        assert_eq!((s.after[0].wins, s.after[0].streak), (Some(3), Some(0)));
        assert_eq!((s.after[1].wins, s.after[1].streak), (Some(0), Some(3)));
        assert!(s.lines.iter().any(|l| l.contains("+2🙏 (dont +1🙏 de bonus de série)")), "{:#?}", s.lines);
        assert!(s.lines.iter().any(|l| l.starts_with("Prime de retour : 3 défaites d'affilée pour Bob : +5🙏")), "{:#?}", s.lines);
        assert!(s.lines.iter().any(|l| l.contains("fin de sa série de 1 victoire d'affilée")), "{:#?}", s.lines);
    }

    #[test]
    fn draw_and_reset() {
        let data = data();
        let a = compo(&data, &[("ushabti", false, 2, 2, 1, 0)], 3, 1, 0);
        let b = compo(&data, &[("sisyphus", false, 0, 3, 0, 0)], 2, 9, 0);
        let compos = [a, b];
        let boards = [compos[0].board(&data), compos[1].board(&data)];
        let res = result(Outcome::Draw, [vec![], vec![]], [1, 1]);
        let s = settle(&data, &names(), &compos, &boards, &res);
        assert_eq!(s.winner, None);
        assert_eq!(s.reset, [true, false]);
        assert_eq!(s.after[0].vp, Some(0));
        assert_eq!(s.after[0].essence, Some(25));
        assert!(s.after[0].units.is_empty());
        assert_eq!(s.after[1].essence, Some(8));
        assert_eq!(s.after[1].vp, Some(2));
    }

    #[test]
    fn quest_heal() {
        let data = data();
        let ankou = data.unit_index("ankou").unwrap();
        let a = compo(&data, &[("ankou", false, 1, 3, 0, 4), ("boar", false, 1, 4, 0, 0)], 0, 20, 0);
        let b = compo(&data, &[("boar", false, 1, 4, 0, 0)], 0, 25, 0);
        let compos = [a, b];
        let boards = [compos[0].board(&data), compos[1].board(&data)];
        let mut res = result(Outcome::WinA, [vec![(data.unit_index("boar").unwrap(), 1)], vec![]], [2, 1]);
        res.deaths[0][0] = 1;
        res.essence[0] = 8;
        let s = settle(&data, &names(), &compos, &boards, &res);
        // 20 + 8 capped at 25.
        assert_eq!(s.after[0].essence, Some(25));
        assert_eq!(s.after[0].units[0].deaths, 5);
        assert_eq!(s.after[0].units[0].id, data.units[ankou].id);
        assert_eq!(s.after[1].essence, Some(24));
    }

    #[test]
    fn replay_is_stable() {
        let data = data();
        let a = "1. 🐗 Sanglier · 1/4/0\n2. 🪖 Hoplite · 1/2/2\n🏆 0 · ✨ 25 · série de 0 défaite";
        let b = "1. 👁️ Cyclope · 3/2/0\n2. 🍄 Korrigan · 2/3/0\n🏆 0 · ✨ 25 · série de 0 défaite";
        let x = challenge(&data, [a, b], [Some("Alice"), Some("Bob")], &[12, 3, 77, 45, 9, 100]).unwrap();
        let y = challenge(&data, [a, b], [Some("Alice"), Some("Bob")], &[12, 3, 77, 45, 9, 100]).unwrap();
        assert_eq!(x.seed, 110_276_440_899);
        assert_eq!(x.forum_text(), y.forum_text());
        assert!(x.forum_text().contains("Seed 110276440899"));
        let err = challenge(&data, [a, "rien"], [None, None], &[1, 2, 3]).err().unwrap();
        assert!(err.iter().any(|e| e.starts_with("Défenseur")));
        assert!(err.iter().any(|e| e.contains("6 dés")));
    }

    /// A fight between two hand-made line-ups, with seed `seed`.
    fn duel(data: &Data, a: &[(&str, bool, i32, i32, i32, u32)], b: &[(&str, bool, i32, i32, i32, u32)], seed: u64) -> Challenge {
        play(data, names(), [compo(data, a, 0, 25, 0), compo(data, b, 0, 25, 0)], vec![1; 6], seed)
    }
    fn log(c: &Challenge) -> String {
        c.log.join("\n")
    }

    #[test]
    fn annihilate_destroys_for_good() {
        let data = data();
        for seed in 0..10 {
            let c = duel(&data, &[("nyarlathotep", false, 5, 7, 1, 0)], &[("saehrimnir", false, 2, 1, 0, 0)], seed);
            let l = log(&c);
            assert!(l.contains("anéantit"), "{l}");
            assert!(!l.contains("renaît"), "Renaissance after Néant: {l}");
            assert!(c.settlement.after[1].units.is_empty());
            assert!(c.settlement.lines.iter().any(|x| x.starts_with("Néant :")));
            assert!(c.settlement.after[0].units.len() == 1);
        }
    }

    #[test]
    fn vengeance_dies_with_its_kill() {
        let data = data();
        for seed in 0..10 {
            let c = duel(&data, &[("erinys", false, 3, 3, 1, 0)], &[("leprechaun", false, 2, 2, 0, 0)], seed);
            assert!(log(&c).contains("Châtiment"), "{}", log(&c));
            assert_eq!(c.result.outcome, Outcome::Draw);
        }
    }

    #[test]
    fn stone_gaze_cancels_every_power() {
        let data = data();
        for seed in 0..10 {
            let c = duel(&data, &[("medusa", false, 2, 4, 0, 0)], &[("draugr", false, 0, 3, 0, 0)], seed);
            let l = log(&c);
            assert!(l.contains("Regard de pierre"), "{l}");
            assert!(!l.contains("Représailles"), "{l}");
            assert!(!l.contains("Éternel retour"), "{l}");
            assert_eq!(c.result.outcome, Outcome::WinA);
        }
    }

    #[test]
    fn draugr_returns_and_dread_weakens() {
        let data = data();
        let c = duel(&data, &[("draugr", false, 0, 3, 0, 0)], &[("vouivre", false, 6, 7, 2, 0)], 1);
        assert!(log(&c).contains("Éternel retour : 🧟 Draugr"), "{}", log(&c));
        let c = duel(&data, &[("bugul_noz", false, 1, 4, 0, 0)], &[("cyclops", false, 3, 2, 0, 0)], 1);
        assert!(log(&c).contains("Effroi : 🌲 Bugul-noz (Alice) → 👁️ Cyclope (Bob) perd 1⚔"), "{}", log(&c));
    }

    #[test]
    fn lucky_star_counts_merged_allies() {
        let data = data();
        let c = duel(
            &data,
            &[("boar", true, 2, 8, 0, 0), ("wedjat", false, 2, 3, 0, 0), ("korrigan", true, 4, 6, 0, 0)],
            &[("boar", false, 1, 4, 0, 0)],
            1,
        );
        assert!(log(&c).contains("+4⚔ +4❤ (2 unités ⭐ sur le terrain)"), "{}", log(&c));
    }

    #[test]
    fn legacy_on_every_death() {
        let data = data();
        let twice = (0..30).any(|seed| {
            let c = duel(&data, &[("saehrimnir", false, 2, 1, 0, 0), ("ushabti", false, 2, 2, 1, 0)], &[("ymir", false, 7, 11, 1, 0)], seed);
            log(&c).matches("Héritage : 🗿 Ouchebti").count() == 2
        });
        assert!(twice, "Sæhrímnir never gave its ATK on both deaths");
    }

    #[test]
    fn pot_of_gold_pays_per_merged_unit() {
        let data = data();
        let mut p = crate::state::Player::new("A".into(), 0, 25);
        let star = |id: &str| {
            let mut u = Unit::new(&data, data.unit_index(id).unwrap());
            u.merged = true;
            u.power = 2;
            u
        };
        p.board = vec![star("boar"), star("korrigan"), Unit::new(&data, data.unit_index("leprechaun").unwrap())];
        crate::shop::income(&data, &mut p);
        assert_eq!(p.favors, data.rules.base_income + 2);
    }
}
