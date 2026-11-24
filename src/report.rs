//! Markdown reports: roster, simulated games, unit duels.

use crate::bot::Strategy;
use crate::data::{Archetype, Data};
use crate::fight::{self, Outcome};
use crate::game::GameResult;
use crate::state::{GameRng, Unit};
use rand::SeedableRng;
use std::fmt::Write;

pub fn roster(data: &Data) -> String {
    let mut out = String::from("| | Unité | Panthéon | Archétype | Rang | Prix | ⚔ | ❤ | 🛡 | Mots-clés |\n|---|---|---|---|---|---|---|---|---|---|\n");
    for (id, u) in data.units.iter().enumerate() {
        let kws: Vec<String> = u.keywords.iter().map(|k| k.to_string()).collect();
        writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            u.emoji, u.name, u.pantheon, u.archetype_label(), u.tier,
            data.cost(id), u.atk, u.hp, u.def, kws.join(" · ")
        )
        .unwrap();
    }
    for s in &data.rules.synergy {
        writeln!(out, "\nSynergie : {} {} ×{} → {}", s.archetype.emoji(), s.archetype, s.count, s.keyword).unwrap();
    }
    out
}

/// Placement statistics for a group of players.
#[derive(Clone)]
struct Tally {
    n: u64,
    place_sum: u64,
    hist: Vec<u64>,
}

impl Tally {
    fn new(players: usize) -> Tally {
        Tally { n: 0, place_sum: 0, hist: vec![0; players] }
    }

    fn add(&mut self, place: usize) {
        self.n += 1;
        self.place_sum += place as u64;
        self.hist[place - 1] += 1;
    }

    fn merge(&mut self, o: &Tally) {
        self.n += o.n;
        self.place_sum += o.place_sum;
        for (a, b) in self.hist.iter_mut().zip(&o.hist) {
            *a += b;
        }
    }

    fn top(&self, k: usize) -> f64 {
        100.0 * self.hist[..k.min(self.hist.len())].iter().sum::<u64>() as f64 / self.n.max(1) as f64
    }

    fn avg(&self) -> f64 {
        self.place_sum as f64 / self.n.max(1) as f64
    }
}

fn tally_table(title: &str, rows: &[(String, Tally)]) -> String {
    let mut out = format!("| {title} | Top 1 | Top 3 | Place moy. | Places 1re→dernière (%) |\n|---|---|---|---|---|\n");
    for (name, t) in rows {
        let hist: Vec<String> = t
            .hist
            .iter()
            .map(|&h| format!("{:.0}", 100.0 * h as f64 / t.n.max(1) as f64))
            .collect();
        writeln!(out, "| {name} | {:.1} % | {:.1} % | {:.2} | {} |", t.top(1), t.top(3), t.avg(), hist.join(" · ")).unwrap();
    }
    out
}

pub fn games(data: &Data, strats: &[Strategy], results: &[GameResult]) -> String {
    let np = data.rules.players;
    let mut by_strat = vec![Tally::new(np); strats.len()];
    let mut unit_owned = vec![0u64; data.units.len()];
    let mut unit_place = vec![0u64; data.units.len()];
    let mut unit_winner = vec![0u64; data.units.len()];
    let mut winner_level = vec![0u64; data.rules.shop_levels.len()];
    let (mut rounds, mut eliminated) = (0u64, 0u64);
    let (mut merges, mut winner_merges, mut merge_ends, mut merge_end_rounds) = (0u64, 0u64, 0u64, 0u64);
    let (mut slots, mut empty_slots, mut rerolls) = (0u64, 0u64, 0u64);
    let (mut star_fights, mut star_wins) = (0u64, 0u64);
    let mut star_holders = Tally::new(np);

    for g in results {
        rounds += g.rounds_played as u64;
        slots += g.slots as u64;
        star_fights += g.top_star_fights as u64;
        star_wins += g.top_star_wins as u64;
        for &place in &g.top_star_places {
            star_holders.add(place);
        }
        empty_slots += g.empty_slots as u64;
        if g.ended_by_merge {
            merge_ends += 1;
            merge_end_rounds += g.rounds_played as u64;
        }
        eliminated += g.eliminated as u64;
        for p in &g.players {
            by_strat[p.strat].add(p.placement);
            merges += p.merges as u64;
            rerolls += p.rerolls as u64;
            if p.placement == 1 {
                winner_level[p.level - 1] += 1;
                winner_merges += p.merges as u64;
            }
            let mut ids = p.board.clone();
            ids.sort_unstable();
            ids.dedup();
            for id in ids {
                unit_owned[id] += 1;
                unit_place[id] += p.placement as u64;
                if p.placement == 1 {
                    unit_winner[id] += 1;
                }
            }
        }
    }

    let games = results.len().max(1) as f64;
    let mut out = String::new();
    writeln!(out, "# Simulation : {} parties, {np} joueurs, {} manches\n", results.len(), data.rules.rounds).unwrap();
    let copies: Vec<String> = (1..=data.by_tier.len() as u8).map(|t| data.pool_copies(t).to_string()).collect();
    writeln!(out, "Exemplaires par unité dans la réserve commune, par rang : {}\n", copies.join(" / ")).unwrap();
    writeln!(
        out,
        "Manches jouées en moyenne : {:.2} · joueurs éliminés en moyenne : {:.2} · part équitable : Top 1 = {:.1} %, place moy. = {:.2}\n",
        rounds as f64 / games,
        eliminated as f64 / games,
        100.0 / np as f64,
        (np + 1) as f64 / 2.0
    )
    .unwrap();

    let group = |key: &dyn Fn(&Strategy) -> String, order: &[String]| -> Vec<(String, Tally)> {
        order
            .iter()
            .map(|label| {
                let mut t = Tally::new(np);
                for (s, st) in strats.iter().zip(&by_strat) {
                    if &key(s) == label {
                        t.merge(st);
                    }
                }
                (label.clone(), t)
            })
            .collect()
    };

    let focus_label = |s: &Strategy| s.focus.map_or("Générique".to_string(), |a| format!("{} {a}", a.emoji()));
    let mut focus_order = vec!["Générique".to_string()];
    focus_order.extend(Archetype::ALL.iter().map(|a| format!("{} {a}", a.emoji())));
    writeln!(
        out,
        "Fusions par joueur : {:.2} (gagnants : {:.2}) · relances par joueur : {:.1} · parties finies par une Ascension : {:.1} % (en moyenne à la manche {:.1})\n",
        merges as f64 / (games * np as f64),
        winner_merges as f64 / games,
        rerolls as f64 / (games * np as f64),
        100.0 * merge_ends as f64 / games,
        merge_end_rounds as f64 / merge_ends.max(1) as f64
    )
    .unwrap();
    writeln!(
        out,
        "⭐ de rang max : détenue en fin de partie par {:.2} % des joueurs (Top 1 {:.1} %, place moy. {:.2}) · combats gagnés en l'alignant : {:.1} % sur {}\n",
        100.0 * star_holders.n as f64 / (games * np as f64),
        star_holders.top(1),
        star_holders.avg(),
        100.0 * star_wins as f64 / star_fights.max(1) as f64,
        star_fights
    )
    .unwrap();
    writeln!(
        out,
        "Emplacements de boutique vides faute d'exemplaires : {:.2} %\n",
        100.0 * empty_slots as f64 / slots.max(1) as f64
    )
    .unwrap();
    out += "## Par archétype\n\n";
    out += &tally_table("Archétype", &group(&focus_label, &focus_order));

    let plans: Vec<String> = ["bas", "moyen", "haut", "ruée"].iter().map(|s| s.to_string()).collect();
    out += "\n## Par plan de boutique\n\nbas = n'investit jamais · moyen = investit ses restes jusqu'au niveau 3, dès la manche 2 · haut = investit ses restes jusqu'au niveau 4, dès la manche 2 · ruée = investit avant d'acheter, jusqu'au niveau 4\n\n";
    let plan_name = |s: &Strategy| s.name.rsplit('-').next().unwrap_or("").to_string();
    out += &tally_table("Plan", &group(&plan_name, &plans));

    let mut rows: Vec<(String, Tally)> = strats.iter().zip(&by_strat).map(|(s, t)| (s.name.clone(), t.clone())).collect();
    rows.sort_by(|a, b| a.1.avg().total_cmp(&b.1.avg()));
    out += "\n## Toutes les stratégies (les meilleures d'abord)\n\n";
    out += &tally_table("Stratégie", &rows);

    out += "\n## Unités sur les terrains finaux (les meilleures d'abord)\n\n| Unité | Rang | Archétype | Sur les terrains | Place moy. des détenteurs | Sur le terrain du gagnant |\n|---|---|---|---|---|---|\n";
    let boards = (results.len() * np).max(1) as f64;
    let mut ids: Vec<usize> = (0..data.units.len()).collect();
    let avg_place = |id: usize| unit_place[id] as f64 / unit_owned[id].max(1) as f64;
    ids.sort_by(|&a, &b| avg_place(a).total_cmp(&avg_place(b)));
    for id in ids {
        let u = &data.units[id];
        writeln!(
            out,
            "| {} {} | {} | {} | {:.1} % | {:.2} | {:.1} % |",
            u.emoji, u.name, u.tier, u.archetype_label(),
            100.0 * unit_owned[id] as f64 / boards,
            avg_place(id),
            100.0 * unit_winner[id] as f64 / games
        )
        .unwrap();
    }

    out += "\n## Synergies sur les terrains finaux\n\n| Synergie | Joueurs qui l'ont | Place moy. | Top 1 |\n|---|---|---|---|\n";
    for syn in &data.rules.synergy {
        let mut t = Tally::new(np);
        for g in results {
            for p in &g.players {
                let n = p.board.iter().filter(|&&id| data.units[id].is(syn.archetype)).count();
                if n >= syn.count {
                    t.add(p.placement);
                }
            }
        }
        writeln!(
            out,
            "| {} {} ×{} | {:.1} % | {:.2} | {:.1} % |",
            syn.archetype.emoji(),
            syn.archetype,
            syn.count,
            100.0 * t.n as f64 / boards,
            t.avg(),
            t.top(1)
        )
        .unwrap();
    }

    out += "\n## Niveau de boutique final du gagnant\n\n| Niveau | Part |\n|---|---|\n";
    for (i, &c) in winner_level.iter().enumerate() {
        writeln!(out, "| {} | {:.1} % |", i + 1, 100.0 * c as f64 / games).unwrap();
    }
    out
}

/// Every unit against every other, alone on its board, `n` fights per pair.
pub fn duels(data: &Data, tier: Option<u8>, n: usize, seed: u64) -> String {
    let ids: Vec<usize> = (0..data.units.len()).filter(|&i| tier.is_none_or(|t| data.units[i].tier == t)).collect();
    let mut rng = GameRng::seed_from_u64(seed);
    let mut out = String::from("Taux de victoire de l'unité en ligne contre l'unité en colonne (un nul compte pour moitié).\n\n|  |");
    for &j in &ids {
        out += &format!(" {} |", data.units[j].emoji);
    }
    out += " Moy. |\n|---|";
    out += &"---|".repeat(ids.len() + 1);
    out += "\n";
    for &i in &ids {
        let d = &data.units[i];
        write!(out, "| {} {} R{} |", d.emoji, d.name, d.tier).unwrap();
        let (mut sum, mut count) = (0.0, 0);
        for &j in &ids {
            if i == j {
                out += " — |";
                continue;
            }
            let mut score = 0.0;
            for _ in 0..n {
                let res = fight::fight(data, ["A", "B"], &[Unit::new(data, i)], &[Unit::new(data, j)], Default::default(), &mut rng, None);
                score += match res.outcome {
                    Outcome::WinA => 1.0,
                    Outcome::Draw => 0.5,
                    Outcome::WinB => 0.0,
                };
            }
            let pct = 100.0 * score / n as f64;
            sum += pct;
            count += 1;
            write!(out, " {pct:.0} |").unwrap();
        }
        writeln!(out, " **{:.0}** |", sum / count.max(1) as f64).unwrap();
    }
    out
}

/// Arena mode report: who wins depending on activity, attitude, archetype and shop plan.
pub fn arena(data: &Data, strats: &[Strategy], results: &[crate::arena::ArenaResult]) -> String {
    use crate::arena::{Activity, ArenaPlayer, Attitude};
    let np = data.rules.players;
    let a = &data.rules.arena;
    let games = results.len().max(1) as f64;
    let all: Vec<&ArenaPlayer> = results.iter().flat_map(|g| &g.players).collect();
    let mean = |v: &[&ArenaPlayer], f: &dyn Fn(&ArenaPlayer) -> f64| v.iter().map(|p| f(p)).sum::<f64>() / v.len().max(1) as f64;
    let mut out = String::new();
    writeln!(out, "# Arène : {} parties, {np} joueurs, {} jours\n", results.len(), a.days).unwrap();
    let pages = results.iter().map(|g| g.pages as f64).sum::<f64>() / games;
    writeln!(
        out,
        "Part équitable : Top 1 = {:.1} %, place moy. = {:.2} · pages du Bazar par partie : {pages:.0} ({:.1} par jour) · matchs nuls par partie : {:.1}\n",
        100.0 / np as f64,
        (np + 1) as f64 / 2.0,
        pages / a.days as f64,
        results.iter().map(|g| g.draws as f64).sum::<f64>() / games,
    )
    .unwrap();
    let winners: Vec<&ArenaPlayer> = all.iter().copied().filter(|p| p.place == 1).collect();
    writeln!(
        out,
        "🏆 du gagnant en moyenne : {:.1} · 🏆 de tous les joueurs : {:.1} · ✨ en fin de partie : {:.1} · remises à zéro par joueur : {:.2} · Ascensions par partie : {:.2} · jours sans combat par joueur : {:.2} · unités anéanties (Néant) par joueur : {:.2}\n",
        mean(&winners, &|p| p.vp as f64),
        mean(&all, &|p| p.vp as f64),
        mean(&all, &|p| p.essence as f64),
        mean(&all, &|p| p.resets as f64),
        all.iter().map(|p| p.ascensions as f64).sum::<f64>() / games,
        mean(&all, &|p| p.idle_days as f64),
        mean(&all, &|p| p.destroyed as f64),
    )
    .unwrap();

    let days = a.days as f64;
    let table = |title: &str, rows: Vec<(String, Vec<&ArenaPlayer>)>| -> String {
        let mut s = format!(
            "| {title} | Top 1 | Place moy. | 🏆 moy. | Combats / jour | Victoires | Exploits | Jours sans combat | Remises à zéro | 🏆 perdus | Messages au Bazar / jour |\n|---|---|---|---|---|---|---|---|---|---|---|\n"
        );
        for (name, v) in rows {
            let top1 = 100.0 * v.iter().filter(|p| p.place == 1).count() as f64 / v.len().max(1) as f64;
            writeln!(
                s,
                "| {name} | {top1:.1} % | {:.2} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} | {:.2} | {:.1} | {:.1} |",
                mean(&v, &|p| p.place as f64),
                mean(&v, &|p| p.vp as f64),
                mean(&v, &|p| p.fights as f64) / days,
                mean(&v, &|p| p.wins as f64),
                mean(&v, &|p| p.upsets as f64),
                mean(&v, &|p| p.idle_days as f64),
                mean(&v, &|p| p.resets as f64),
                mean(&v, &|p| p.vp_lost as f64),
                mean(&v, &|p| p.posts as f64) / days,
            )
            .unwrap();
        }
        s
    };

    out += "## Par présence\n\n";
    out += &table(
        "Présence",
        Activity::ALL.iter().map(|&x| (x.label().to_string(), all.iter().copied().filter(|p| p.activity == x).collect())).collect(),
    );
    out += "\n## Par attitude\n\n";
    out += &table(
        "Attitude",
        Attitude::ALL.iter().map(|&x| (x.label().to_string(), all.iter().copied().filter(|p| p.attitude == x).collect())).collect(),
    );
    out += "\n## Présence × attitude\n\n";
    let mut rows = Vec::new();
    for act in Activity::ALL {
        for att in Attitude::ALL {
            let name = format!("{} · {}", act.label().split(' ').next().unwrap(), att.label().split(' ').next().unwrap());
            rows.push((name, all.iter().copied().filter(|p| p.activity == act && p.attitude == att).collect()));
        }
    }
    out += &table("Profil", rows);

    let focus_label = |s: &Strategy| s.focus.map_or("Générique".to_string(), |x| format!("{} {x}", x.emoji()));
    let mut focus_order = vec!["Générique".to_string()];
    focus_order.extend(Archetype::ALL.iter().map(|x| format!("{} {x}", x.emoji())));
    out += "\n## Par archétype\n\n";
    out += &table(
        "Archétype",
        focus_order.iter().map(|f| (f.clone(), all.iter().copied().filter(|p| &focus_label(&strats[p.strat]) == f).collect())).collect(),
    );
    out += "\n## Par plan de boutique\n\nbas = n'investit jamais · moyen = investit jusqu'au niveau 3 · haut = investit jusqu'au niveau 4\n\n";
    out += &table(
        "Plan",
        ["bas", "moyen", "haut"]
            .iter()
            .map(|&pl| (pl.to_string(), all.iter().copied().filter(|p| strats[p.strat].name.ends_with(pl)).collect()))
            .collect(),
    );

    out += "\n## Le meneur deux jours avant la fin\n\n| Attitude du meneur | Cas | Gagne la partie |\n|---|---|---|\n";
    for att in Attitude::ALL {
        let cases: Vec<bool> = results.iter().filter_map(|g| g.late_leader).filter(|l| l.0 == att).map(|l| l.1).collect();
        writeln!(
            out,
            "| {} | {:.1} % des parties | {:.1} % |",
            att.label(),
            100.0 * cases.len() as f64 / games,
            100.0 * cases.iter().filter(|&&w| w).count() as f64 / cases.len().max(1) as f64
        )
        .unwrap();
    }

    out += "\n## Niveau de boutique final du gagnant\n\n| Niveau | Part |\n|---|---|\n";
    for lvl in 1..=data.rules.shop_levels.len() {
        writeln!(out, "| {lvl} | {:.1} % |", 100.0 * winners.iter().filter(|p| p.level == lvl).count() as f64 / games).unwrap();
    }

    out += "\n## Unités sur les terrains finaux (les meilleures d'abord)\n\n| Unité | Rang | Archétype | Sur les terrains | Place moy. des détenteurs | Sur le terrain du gagnant |\n|---|---|---|---|---|---|\n";
    let mut owned = vec![0u64; data.units.len()];
    let mut place = vec![0u64; data.units.len()];
    let mut won = vec![0u64; data.units.len()];
    for p in &all {
        let mut ids = p.board.clone();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            owned[id] += 1;
            place[id] += p.place as u64;
            won[id] += (p.place == 1) as u64;
        }
    }
    let avg = |id: usize| place[id] as f64 / owned[id].max(1) as f64;
    let mut ids: Vec<usize> = (0..data.units.len()).collect();
    ids.sort_by(|&x, &y| avg(x).total_cmp(&avg(y)));
    for id in ids {
        let u = &data.units[id];
        writeln!(
            out,
            "| {} {} | {} | {} | {:.1} % | {:.2} | {:.1} % |",
            u.emoji,
            u.name,
            u.tier,
            u.archetype_label(),
            100.0 * owned[id] as f64 / all.len().max(1) as f64,
            avg(id),
            100.0 * won[id] as f64 / games
        )
        .unwrap();
    }
    out
}
