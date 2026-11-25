//! A player's post in the Bazar: the orders (sell, invest, buy), the player's state after them
//! and the text to paste on the forum, plus the reader that turns such a post back into a state.
//!
//! ```text
//! [b]acheter Korrigan · vendre Géant du givre · investir 2[/b]
//! Terrain : 🐍 Méduse 2/4/0 · 🍄 Korrigan 2/3/0
//! Réserve : —
//! Faveurs : 4🙏 · niveau 1 (2/5🙏 investis vers le niveau 2)
//! ```
//!
//! Random effects (the reward of a merge, Escorte, Récupération) are not decided here: they
//! are written in the post for the GM.

use crate::compo::{self, Composition, Entry, LineKind};
use crate::data::{Data, Keyword};
use crate::shop;
use crate::state::Unit;
use serde::{Deserialize, Serialize};

/// What a player owns in the Bazar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub favors: i32,
    /// Shop level, from 1.
    #[serde(default = "first_level")]
    pub level: usize,
    /// Favors invested towards the next level.
    #[serde(default)]
    pub invested: i32,
    #[serde(default)]
    pub board: Vec<Entry>,
    #[serde(default)]
    pub reserve: Vec<Entry>,
}

fn first_level() -> usize {
    1
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState { name: String::new(), favors: 0, level: 1, invested: 0, board: Vec::new(), reserve: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Board,
    Reserve,
}

/// A unit sold, by its place in the state the orders start from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sell {
    pub place: Place,
    pub index: usize,
}

/// The orders of one Bazar post. No purchase means `passer`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Orders {
    /// Unit id bought, from the shop of the player's level once the investment is made.
    #[serde(default)]
    pub buy: Option<String>,
    #[serde(default)]
    pub sells: Vec<Sell>,
    #[serde(default)]
    pub invest: i32,
}

/// One level's shop, as the GM tool publishes it: the unit ids still on offer.
#[derive(Debug, Clone, Deserialize)]
pub struct LevelShop {
    pub level: usize,
    pub units: Vec<String>,
}

/// What the orders do.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub state: PlayerState,
    pub errors: Vec<String>,
    /// Lines for the GM, written under the state in the post.
    pub notes: Vec<String>,
    /// Favors: post fee, unit price, sales, invested.
    pub fee: i32,
    pub price: i32,
    pub sales: i32,
    pub invested: i32,
    /// The unit taken from the common reserve.
    pub taken: Option<String>,
    /// Copies going back to the common reserve (sales, Ascension): unit id per copy.
    pub returned: Vec<String>,
    /// Units merged into a ⭐ unit (one id per merge).
    pub merged: Vec<String>,
    pub ascension: bool,
    /// The forum post (empty when there are errors).
    pub text: String,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

fn to_unit(data: &Data, e: &Entry) -> Option<Unit> {
    let idx = data.unit_index(&e.id)?;
    let needed = compo::offering_needed(data, idx);
    Some(Unit {
        id: idx,
        atk: e.atk,
        hp: e.hp,
        def: e.def,
        rebirths: 0,
        deaths: e.deaths,
        quest_done: needed.is_some_and(|n| e.deaths >= n),
        merged: e.star,
        power: compo::power(data, idx, e.star),
    })
}

fn max_level(data: &Data) -> usize {
    data.rules.arena.bazar_odds.len()
}

/// "⭐🐍 Méduse"
fn unit_name(data: &Data, e: &Entry) -> String {
    match data.unit_index(&e.id) {
        Some(i) => format!("{}{} {}", if e.star { "⭐" } else { "" }, data.units[i].emoji, data.units[i].name),
        None => e.id.clone(),
    }
}

/// Problems with a state itself: level, favors, sizes, unknown units.
pub fn check_state(data: &Data, s: &PlayerState) -> Vec<String> {
    let r = &data.rules;
    let mut errors = Vec::new();
    if s.level < 1 || s.level > max_level(data) {
        errors.push(format!("Niveau de boutique invalide : {} (de 1 à {})", s.level, max_level(data)));
    }
    if s.favors < 0 || s.favors > 100_000 {
        errors.push(format!("Faveurs invalides : {}", s.favors));
    }
    if s.invested < 0 || (s.level >= 1 && s.level < max_level(data) && s.invested >= r.upgrade_threshold[s.level - 1]) {
        errors.push(format!("Faveurs investies invalides : {}", s.invested));
    }
    if s.board.len() > r.board_size {
        errors.push(format!("Terrain : {} unités, au plus {}", s.board.len(), r.board_size));
    }
    if s.reserve.len() > r.reserve_size {
        errors.push(format!("Réserve : {} unités, au plus {}", s.reserve.len(), r.reserve_size));
    }
    for e in s.board.iter().chain(&s.reserve) {
        if data.unit_index(&e.id).is_none() {
            errors.push(format!("Unité inconnue : {}", e.id));
        } else if e.hp < 1 || [e.atk, e.hp, e.def].iter().any(|&v| !(0..=compo::MAX_STAT).contains(&v)) {
            errors.push(format!("{} : statistiques invalides", unit_name(data, e)));
        }
    }
    errors
}

/// Applies the orders of a post to `state`: post fee, sales, investment, purchase (in the
/// shop of the level reached), then merges. `shops` lists the units still on offer per level;
/// without it the purchase isn't checked against a shop.
pub fn apply(data: &Data, state: &PlayerState, orders: &Orders, shops: Option<&[LevelShop]>) -> Outcome {
    let r = &data.rules;
    let mut out = Outcome { errors: check_state(data, state), ..Outcome::default() };
    if !out.ok() {
        return out;
    }
    let mut s = state.clone();
    out.fee = r.arena.post_cost;
    s.favors -= out.fee;

    // Sales.
    let mut sold = [vec![false; s.board.len()], vec![false; s.reserve.len()]];
    for sell in &orders.sells {
        let (list, k) = match sell.place {
            Place::Board => (&state.board, 0),
            Place::Reserve => (&state.reserve, 1),
        };
        let Some(e) = list.get(sell.index) else {
            out.errors.push(format!("Vente : aucune unité à la place {} {}", sell.index + 1, if k == 0 { "du terrain" } else { "de la réserve" }));
            continue;
        };
        if std::mem::replace(&mut sold[k][sell.index], true) {
            out.errors.push(format!("{} est vendue deux fois", unit_name(data, e)));
            continue;
        }
        s.favors += r.sell_value;
        out.sales += r.sell_value;
        let idx = data.unit_index(&e.id).unwrap();
        let copies = if e.star { r.merge_copies } else { 1 };
        out.returned.extend(std::iter::repeat_n(e.id.clone(), copies));
        let m = compo::power(data, idx, e.star);
        for kw in &data.units[idx].keywords {
            match kw.scaled(m) {
                Keyword::Escort { atk, hp } => out.notes.push(format!(
                    "Escorte : {} est vendue, un allié du terrain au hasard gagne {} (tirage du MJ)",
                    unit_name(data, e),
                    crate::data::bonus(atk, hp, 0)
                )),
                Keyword::Salvage { max } => out.notes.push(format!(
                    "Récupération : {} est vendue, +1🙏 par retour à la vie de la partie, jusqu'à {max}🙏 (à recevoir du MJ)",
                    unit_name(data, e)
                )),
                _ => {}
            }
        }
    }
    let keep = |list: &[Entry], flags: &[bool]| list.iter().zip(flags).filter(|(_, f)| !**f).map(|(e, _)| e.clone()).collect::<Vec<_>>();
    s.board = keep(&state.board, &sold[0]);
    s.reserve = keep(&state.reserve, &sold[1]);

    // Investment.
    if orders.invest < 0 {
        out.errors.push("On ne peut pas investir un nombre négatif de faveurs".to_string());
    } else if orders.invest > 0 {
        let top = max_level(data);
        let needed: i32 = (s.level..top).map(|l| r.upgrade_threshold[l - 1]).sum::<i32>() - s.invested;
        if needed <= 0 {
            out.errors.push(format!("Votre boutique est déjà au niveau maximum ({top})"));
        } else if orders.invest > needed {
            out.errors.push(format!("{needed}🙏 suffisent pour atteindre le niveau {top} : n'investissez pas plus"));
        } else {
            s.favors -= orders.invest;
            out.invested = orders.invest;
            s.invested += orders.invest;
            while s.level < top && s.invested >= r.upgrade_threshold[s.level - 1] {
                s.invested -= r.upgrade_threshold[s.level - 1];
                s.level += 1;
                out.notes.push(format!("⬆️ Boutique au niveau {}", s.level));
            }
            if s.level >= top {
                s.invested = 0;
            }
        }
    }

    // Purchase.
    if let Some(id) = &orders.buy {
        match data.unit_index(id) {
            None => out.errors.push(format!("Unité inconnue : {id}")),
            Some(idx) => {
                let d = &data.units[idx];
                let offered = shops.map(|list| list.iter().any(|sh| sh.level == s.level && sh.units.iter().any(|u| u == id)));
                if offered == Some(false) {
                    out.errors.push(format!("{} {} n'est pas en vente dans la boutique de niveau {}", d.emoji, d.name, s.level));
                }
                let copies = s.board.iter().chain(&s.reserve).filter(|e| e.id == *id && !e.star).count();
                let room = s.board.len() < r.board_size || s.reserve.len() < r.reserve_size;
                if !room && copies + 1 < r.merge_copies {
                    out.errors.push("Plus de place sur le terrain ni en réserve : vendez une unité d'abord".to_string());
                }
                out.price = data.cost(idx) - r.arena.post_cost;
                s.favors -= out.price;
                let e = Entry { id: id.clone(), star: false, atk: d.atk, hp: d.hp, def: d.def, deaths: 0 };
                if s.board.len() < r.board_size { s.board.push(e) } else { s.reserve.push(e) }
                out.taken = Some(id.clone());
            }
        }
    }
    if s.favors < 0 {
        out.errors.push(format!(
            "Il manque {}🙏 : ce message coûte {}🙏 (message {}🙏{}{}){}, vous en avez {}🙏",
            -s.favors,
            out.fee + out.price + out.invested,
            out.fee,
            if out.price > 0 { format!(" + unité {}🙏", out.price) } else { String::new() },
            if out.invested > 0 { format!(" + investissement {}🙏", out.invested) } else { String::new() },
            if out.sales > 0 { format!(", ventes +{}🙏", out.sales) } else { String::new() },
            state.favors,
        ));
    }
    if !out.ok() {
        return out;
    }

    merge(data, &mut s, &mut out);
    if s.board.len() > r.board_size || s.reserve.len() > r.reserve_size {
        out.errors.push("Plus de place sur le terrain ni en réserve : vendez une unité d'abord".to_string());
        return out;
    }
    out.state = s;
    out.text = post_text(data, state, orders, &out);
    out
}

/// Merges every set of identical copies, the ⭐ unit taking the place of the first copy.
fn merge(data: &Data, s: &mut PlayerState, out: &mut Outcome) {
    let r = &data.rules;
    loop {
        let all: Vec<&Entry> = s.board.iter().chain(&s.reserve).filter(|e| !e.star).collect();
        let Some(id) = all.iter().map(|e| e.id.clone()).find(|id| all.iter().filter(|e| e.id == *id).count() >= r.merge_copies) else {
            break;
        };
        let mut copies = Vec::new();
        let mut first_board: Option<usize> = None;
        for (k, list) in [&mut s.board, &mut s.reserve].into_iter().enumerate() {
            let mut i = 0;
            while i < list.len() && copies.len() < r.merge_copies {
                if list[i].id == id && !list[i].star {
                    if k == 0 && first_board.is_none() {
                        first_board = Some(i);
                    }
                    copies.push(list.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        let units: Vec<Unit> = copies.iter().filter_map(|e| to_unit(data, e)).collect();
        let star = shop::combine(data, &units);
        let entry = Composition::entries(data, std::slice::from_ref(&star)).remove(0);
        let d = &data.units[star.id];
        out.merged.push(id.clone());
        if d.tier as usize >= data.by_tier.len() {
            out.ascension = true;
            out.returned.extend(std::iter::repeat_n(id.clone(), r.merge_copies));
            out.notes.push(format!(
                "🌟 Ascension : trois {} {} rejoignent les dieux, +{}🏆 (les exemplaires retournent à la réserve commune)",
                d.emoji, d.name, r.arena.ascension_vp
            ));
            continue;
        }
        out.notes.push(format!(
            "🎁 Fusion : ⭐{} {} · une unité de rang {} au hasard, à recevoir du MJ",
            d.emoji,
            d.name,
            d.tier + 1
        ));
        match first_board {
            Some(i) => s.board.insert(i, entry),
            None if s.board.len() < r.board_size => s.board.push(entry),
            None => s.reserve.push(entry),
        }
    }
}

/// "⭐🐍 Méduse 4/8/0", "⚰️ Ankou 1/3/0 (Offrande 2/5)"
fn token(data: &Data, e: &Entry) -> String {
    let mut t = format!("{} {}/{}/{}", unit_name(data, e), e.atk, e.hp, e.def);
    if let Some(n) = data.unit_index(&e.id).and_then(|i| compo::offering_needed(data, i)) {
        t += &format!(" (Offrande {}/{n})", e.deaths.min(n));
    }
    t
}

fn units_line(data: &Data, label: &str, list: &[Entry]) -> String {
    let v: Vec<String> = list.iter().map(|e| token(data, e)).collect();
    format!("{label} : {}", if v.is_empty() { "—".to_string() } else { v.join(" · ") })
}

/// "4🙏 · niveau 1 (2/5🙏 investis vers le niveau 2)"
fn favors_line(data: &Data, s: &PlayerState) -> String {
    let mut line = format!("Faveurs : {}🙏 · niveau {}", s.favors, s.level);
    if s.invested > 0 && s.level < max_level(data) {
        line += &format!(" ({}/{}🙏 investis vers le niveau {})", s.invested, data.rules.upgrade_threshold[s.level - 1], s.level + 1);
    }
    line
}

/// The post: orders in bold, then the state after them and the notes for the GM.
pub fn post_text(data: &Data, before: &PlayerState, orders: &Orders, out: &Outcome) -> String {
    let mut parts = Vec::new();
    match orders.buy.as_ref().and_then(|id| data.unit_index(id)) {
        Some(i) => parts.push(format!("acheter {}", data.units[i].name)),
        None => parts.push("passer".to_string()),
    }
    for sell in &orders.sells {
        let list = if sell.place == Place::Board { &before.board } else { &before.reserve };
        if let Some(e) = list.get(sell.index).and_then(|e| data.unit_index(&e.id).map(|i| (e, i))) {
            parts.push(format!("vendre {}{}", data.units[e.1].name, if e.0.star { " ⭐" } else { "" }));
        }
    }
    if orders.invest > 0 {
        parts.push(format!("investir {}", orders.invest));
    }
    let s = &out.state;
    let mut lines = vec![
        format!("[b]{}[/b]", parts.join(" · ")),
        units_line(data, "Terrain", &s.board),
        units_line(data, "Réserve", &s.reserve),
        favors_line(data, s),
    ];
    lines.extend(out.notes.iter().map(|n| format!("[i]{n}[/i]")));
    lines.join("\n")
}

/// A state read back from a post.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ParsedPost {
    pub state: PlayerState,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Reads the "Terrain", "Réserve" and "Faveurs" lines of a Bazar post (the last ones, when
/// several posts are pasted).
pub fn parse_post(data: &Data, text: &str) -> ParsedPost {
    let ms = compo::matchers(data);
    let mut p = ParsedPost::default();
    let (mut board, mut reserve, mut favors) = (None, None, None);
    for raw in compo::normalize(text).lines() {
        let line = raw.trim();
        let folded = compo::fold(line);
        let Some(colon) = line.find(':') else { continue };
        let rest = line[colon + 1..].trim();
        let head = compo::fold(&line[..colon]);
        let head = head.trim();
        if head == "terrain" {
            board = Some(rest.to_string());
        } else if head == "reserve" {
            reserve = Some(rest.to_string());
        } else if head == "faveurs" {
            favors = Some(folded[folded.find(':').unwrap() + 1..].to_string());
        }
    }
    let read_units = |label: &str, text: Option<String>, out: &mut Vec<Entry>, p: &mut ParsedPost| {
        let Some(text) = text else {
            p.warnings.push(format!("Ligne « {label} » introuvable : comptée vide"));
            return;
        };
        let mut rank = 0;
        for tok in text.split('·').map(str::trim).filter(|t| !t.is_empty() && !["—", "-", "–", "aucune", "rien"].contains(t)) {
            rank += 1;
            match compo::unit_line(data, &ms, tok, rank) {
                LineKind::Unit(mut e, offering) => {
                    if let Some(d) = offering {
                        let needed = data.unit_index(&e.id).and_then(|i| compo::offering_needed(data, i)).unwrap_or(0);
                        e.deaths = (d as u32).min(needed);
                    }
                    out.push(e);
                }
                LineKind::Error(m) => p.errors.push(format!("{label} : {}", m.trim_start_matches(&format!("Unité {rank} ")[..]))),
                LineKind::Ignored(_) | LineKind::Other => p.errors.push(format!("{label} : unité illisible « {tok} »")),
            }
        }
    };
    let mut s = PlayerState::default();
    read_units("Terrain", board, &mut s.board, &mut p);
    read_units("Réserve", reserve, &mut s.reserve, &mut p);
    match favors {
        None => p.errors.push("Ligne « Faveurs » introuvable".to_string()),
        Some(f) => {
            let nums = compo::numbers(&f);
            match nums.first() {
                Some(n) => s.favors = n.2 as i32,
                None => p.errors.push("Faveurs : nombre illisible".to_string()),
            }
            if let Some(at) = f.find("niveau") {
                if let Some(n) = compo::numbers(&f[at..]).first() {
                    s.level = n.2 as usize;
                }
            } else {
                p.warnings.push("Niveau de boutique absent : compté 1".to_string());
            }
            if let Some(open) = f.find('(') {
                if let Some(n) = compo::numbers(&f[open..]).first() {
                    s.invested = n.2 as i32;
                }
            }
        }
    }
    p.errors.extend(check_state(data, &s));
    p.state = s;
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Data {
        Data::from_strs(include_str!("../data/units.toml"), include_str!("../data/rules.toml")).unwrap()
    }

    fn base(data: &Data, id: &str) -> Entry {
        let d = &data.units[data.unit_index(id).unwrap()];
        Entry { id: id.into(), star: false, atk: d.atk, hp: d.hp, def: d.def, deaths: 0 }
    }

    fn state(favors: i32, board: Vec<Entry>, reserve: Vec<Entry>) -> PlayerState {
        PlayerState { favors, board, reserve, ..PlayerState::default() }
    }

    #[test]
    fn buy_costs_post_and_price() {
        let d = data();
        let s = state(10, vec![base(&d, "frost_giant")], vec![]);
        let o = apply(&d, &s, &Orders { buy: Some("medusa".into()), ..Orders::default() }, None);
        assert!(o.ok(), "{:?}", o.errors);
        let tier = d.units[d.unit_index("medusa").unwrap()].tier as usize;
        assert_eq!(o.state.favors, 10 - d.rules.tier_cost[tier - 1]);
        assert_eq!(o.state.board.len(), 2);
        assert_eq!(o.taken.as_deref(), Some("medusa"));
        assert!(o.text.starts_with("[b]acheter Méduse[/b]\nTerrain : "), "{}", o.text);
    }

    #[test]
    fn pass_costs_the_post_only() {
        let d = data();
        let o = apply(&d, &state(3, vec![], vec![]), &Orders::default(), None);
        assert_eq!(o.state.favors, 3 - d.rules.arena.post_cost);
        assert!(o.text.starts_with("[b]passer[/b]"));
        let o = apply(&d, &state(0, vec![], vec![]), &Orders::default(), None);
        assert!(!o.ok());
    }

    #[test]
    fn invest_levels_up_then_buys_in_the_new_shop() {
        let d = data();
        let t = d.rules.upgrade_threshold[0];
        let mut s = state(20, vec![], vec![]);
        s.invested = t - 2;
        let shops = vec![LevelShop { level: 1, units: vec!["boar".into()] }, LevelShop { level: 2, units: vec!["hydra".into()] }];
        let o = apply(&d, &s, &Orders { buy: Some("hydra".into()), invest: 5, ..Orders::default() }, Some(&shops));
        assert!(o.ok(), "{:?}", o.errors);
        assert_eq!(o.state.level, 2);
        assert_eq!(o.state.invested, 3, "the excess carries over");
        let o = apply(&d, &s, &Orders { buy: Some("boar".into()), invest: 5, ..Orders::default() }, Some(&shops));
        assert!(!o.ok(), "the level 1 shop is gone once level 2 is reached");
    }

    #[test]
    fn merge_from_board_and_reserve_without_room() {
        let d = data();
        let mut boar = base(&d, "boar");
        boar.atk += 2; // a permanent bonus is kept
        let board = vec![base(&d, "boar"), base(&d, "medusa"), boar, base(&d, "korrigan")];
        let reserve: Vec<Entry> = ["ankou", "draugr", "cyclops", "ka"].iter().map(|id| base(&d, id)).collect();
        let s = state(10, board, reserve);
        let o = apply(&d, &s, &Orders { buy: Some("boar".into()), ..Orders::default() }, None);
        assert!(o.ok(), "{:?}", o.errors);
        assert_eq!(o.merged, vec!["boar".to_string()]);
        let star = &o.state.board[0];
        let def = &d.units[d.unit_index("boar").unwrap()];
        assert!(star.star && star.id == "boar");
        assert_eq!(star.atk, def.atk * d.rules.merge_power[0] + 2);
        assert_eq!((o.state.board.len(), o.state.reserve.len()), (3, 4));
        assert!(o.text.contains("🎁 Fusion"), "{}", o.text);
    }

    #[test]
    fn no_room_no_buy() {
        let d = data();
        let four = |ids: [&str; 4]| ids.iter().map(|id| base(&d, id)).collect::<Vec<_>>();
        let s = state(10, four(["boar", "medusa", "frost_giant", "korrigan"]), four(["ankou", "draugr", "cyclops", "ka"]));
        let o = apply(&d, &s, &Orders { buy: Some("hydra".into()), ..Orders::default() }, None);
        assert!(!o.ok());
        let o = apply(&d, &s, &Orders { buy: Some("hydra".into()), sells: vec![Sell { place: Place::Reserve, index: 1 }], ..Orders::default() }, None);
        assert!(o.ok(), "{:?}", o.errors);
        assert_eq!(o.returned, vec!["draugr".to_string()]);
        assert!(o.text.contains("vendre Draugr"), "{}", o.text);
    }

    #[test]
    fn post_reads_back() {
        let d = data();
        let mut ankou = base(&d, "ankou");
        ankou.deaths = 2;
        let mut star = base(&d, "medusa");
        star.star = true;
        star.atk *= 2;
        star.hp *= 2;
        let mut s = state(9, vec![star, ankou], vec![base(&d, "valkyrie")]);
        s.invested = 1;
        let o = apply(&d, &s, &Orders { invest: 2, ..Orders::default() }, None);
        assert!(o.ok(), "{:?}", o.errors);
        let quoted = format!("[quote=Alice]\n{}\n[/quote]", o.text);
        let p = parse_post(&d, &quoted);
        assert!(p.errors.is_empty(), "{:?}\n{}", p.errors, o.text);
        assert_eq!(p.state, o.state);
    }
}
