//! Arena compositions as players post them on the forum: the text format, a lenient parser
//! with French errors and warnings, and the formatter the composer uses.
//!
//! ```text
//! [b]⚔ Composition de Alice[/b] · 🏆 3 · ✨ 18 · série de 2 défaites
//! 1. 🐗 Sanglier · 1/5/0
//! 2. ⭐🪖 Hoplite · 2/4/4
//! 3. ⚰️ Ankou · 1/3/0 · Offrande 3/5
//! [i]Stats : ⚔/❤/🛡[/i]
//! {d100} {d100} {d100}
//! ```

use crate::data::{Data, Keyword};
use crate::state::Unit;
use serde::{Deserialize, Serialize};

/// Highest stat value accepted.
pub const MAX_STAT: i32 = 999;

/// One unit of a composition, as written by its player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Unit id ("boar").
    pub id: String,
    /// ⭐ unit (merged copies).
    #[serde(default)]
    pub star: bool,
    pub atk: i32,
    pub hp: i32,
    pub def: i32,
    /// Offering quest: deaths counted so far (reaching the target means the quest paid out).
    #[serde(default)]
    pub deaths: u32,
}

/// A player's composition: units from left to right, plus the numbers the bilan needs.
/// A number missing from the text stays None.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Composition {
    #[serde(default)]
    pub name: String,
    /// Victory points (🏆).
    #[serde(default)]
    pub vp: Option<i32>,
    /// Divine essence (✨).
    #[serde(default)]
    pub essence: Option<i32>,
    /// Defeats in a row (Déchaînement, comeback prime).
    #[serde(default)]
    pub streak: Option<i32>,
    /// Wins in a row (streak bonus).
    #[serde(default)]
    pub wins: Option<i32>,
    #[serde(default)]
    pub units: Vec<Entry>,
}

/// What the parser read, with what it couldn't (errors) or wasn't sure of (warnings).
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub compo: Composition,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// The data fingerprint written in the composition footer, if any.
    pub data_hash: Option<String>,
}

impl Parsed {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Deaths the unit's Offering quest needs, if it has one.
pub fn offering_needed(data: &Data, idx: usize) -> Option<u32> {
    data.units[idx].keywords.iter().find_map(|k| match *k {
        Keyword::Offering { deaths, .. } => Some(deaths),
        _ => None,
    })
}

/// Keyword multiplier of a unit: merge_power of its tier for a ⭐ unit, else 1.
pub fn power(data: &Data, idx: usize, star: bool) -> i32 {
    if star { data.rules.merge_power[data.units[idx].tier as usize - 1] } else { 1 }
}

/// Base stats of a unit, times merge_power for a ⭐ unit.
pub fn base_stats(data: &Data, idx: usize, star: bool) -> (i32, i32, i32) {
    let d = &data.units[idx];
    let p = power(data, idx, star);
    (d.atk * p, d.hp * p, d.def * p)
}

impl Composition {
    /// The board as the engine sees it. Unknown ids are skipped (`parse` reports them).
    pub fn board(&self, data: &Data) -> Vec<Unit> {
        self.units
            .iter()
            .filter_map(|e| {
                let idx = data.unit_index(&e.id)?;
                let needed = offering_needed(data, idx);
                Some(Unit {
                    id: idx,
                    atk: e.atk,
                    hp: e.hp,
                    def: e.def,
                    rebirths: 0,
                    deaths: e.deaths,
                    quest_done: needed.is_some_and(|n| e.deaths >= n),
                    merged: e.star,
                    power: power(data, idx, e.star),
                })
            })
            .collect()
    }

    /// Entries from an engine board, the Offering progress capped at its target.
    pub fn entries(data: &Data, board: &[Unit]) -> Vec<Entry> {
        board
            .iter()
            .map(|u| Entry {
                id: data.units[u.id].id.clone(),
                star: u.merged,
                atk: u.atk,
                hp: u.hp,
                def: u.def,
                deaths: offering_needed(data, u.id).map_or(0, |n| u.deaths.min(n)),
            })
            .collect()
    }
}

/// Keeps a player name from breaking the format.
fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '[' | ']' | '·' | '\n' | '\r' | '{' | '}')).collect::<String>().trim().to_string()
}

/// The composition as a forum post, ready to paste (with its three dice).
pub fn format(data: &Data, c: &Composition) -> String {
    let name = clean_name(&c.name);
    let mut head = if name.is_empty() { "[b]⚔ Composition[/b]".to_string() } else { format!("[b]⚔ Composition {}[/b]", crate::forum::de(&name)) };
    if let Some(v) = c.vp {
        head += &format!(" · 🏆 {v}");
    }
    if let Some(e) = c.essence {
        head += &format!(" · ✨ {e}");
    }
    if let Some(s) = c.streak {
        head += &format!(" · 💔 {s} défaite{} d'affilée", if s > 1 { "s" } else { "" });
    }
    if let Some(w) = c.wins {
        head += &format!(" · 🔥 {w} victoire{} d'affilée", if w > 1 { "s" } else { "" });
    }
    let mut lines = vec![head];
    for (k, e) in c.units.iter().enumerate() {
        let Some(idx) = data.unit_index(&e.id) else { continue };
        let d = &data.units[idx];
        let star = if e.star { "⭐" } else { "" };
        let mut line = format!("{}. {star}{} {} · {}/{}/{}", k + 1, d.emoji, d.name, e.atk, e.hp, e.def);
        if let Some(n) = offering_needed(data, idx) {
            line += &format!(" · Offrande {}/{n}", e.deaths.min(n));
        }
        lines.push(line);
    }
    lines.push("[i]Stats : ⚔/❤/🛡[/i]".to_string());
    lines.push("{d100} {d100} {d100}".to_string());
    lines.join("\n")
}

/// Lowercase, without accents: "Heiðrún" → "heidrun", "Érinye" → "erinye".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ð' => out.push('d'),
            'þ' => out.push_str("th"),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'ß' => out.push_str("ss"),
            '’' | '‘' | '`' => out.push('\''),
            '\u{fe0f}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Removes BBCode tags ([b], [/i], [quote=…]…), carriage returns and emoji variation selectors.
fn normalize(text: &str) -> String {
    let text = text.replace(['\r', '\u{fe0f}'], "");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let tag = after.find(']').filter(|&close| {
            let inner = &after[..close];
            let name = inner.trim_start_matches('/');
            !inner.contains('\n') && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '*')
        });
        match tag {
            Some(close) => rest = &after[close + 1..],
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Runs of ASCII digits in `s`, as (start byte, end byte, value capped at 1,000,000).
fn numbers(s: &str) -> Vec<(usize, usize, u64)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            let mut v: u64 = 0;
            while i < b.len() && b[i].is_ascii_digit() {
                v = (v * 10 + (b[i] - b'0') as u64).min(1_000_000);
                i += 1;
            }
            out.push((start, i, v));
        } else {
            i += 1;
        }
    }
    out
}

/// The first "a/b/c" in `s` (spaces allowed around the slashes): (start, end, values).
fn find_triple(s: &str) -> Option<(usize, usize, [u64; 3])> {
    let nums = numbers(s);
    let slash = |from: usize, to: usize| s[from..to].trim() == "/";
    nums.windows(3)
        .find(|w| slash(w[0].1, w[1].0) && slash(w[1].1, w[2].0))
        .map(|w| (w[0].0, w[2].1, [w[0].2, w[1].2, w[2].2]))
}

/// "Offrande 3/5" (any case, accents optional): the deaths counted, if written.
fn find_offering(folded: &str) -> Option<u64> {
    let at = folded.find("offrande")?;
    let rest = &folded[at + "offrande".len()..];
    let nums = numbers(rest);
    let first = nums.first()?;
    rest[..first.0].trim_matches(|c: char| c.is_whitespace() || c == ':').is_empty().then_some(first.2)
}

/// The number right after `marker` (skipping spaces and ':'), or else right before it.
fn number_near(s: &str, marker: &str) -> Option<i32> {
    let at = s.find(marker)?;
    let after = &s[at + marker.len()..];
    let nums = numbers(after);
    if let Some(n) = nums.first().filter(|n| after[..n.0].trim_matches(|c: char| c.is_whitespace() || c == ':').is_empty()) {
        return Some(n.2 as i32);
    }
    let before = &s[..at];
    let nums = numbers(before);
    nums.last().filter(|n| before[n.1..].trim().is_empty()).map(|n| n.2 as i32)
}

/// Removes a leading "1.", "1)", "#1", "-", "*" or "•" from a unit line.
fn strip_enumerator(line: &str) -> (&str, bool) {
    let t = line.trim_start();
    let t2 = t.trim_start_matches('#');
    let digits = t2.len() - t2.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let rest = &t2[digits..];
        let rest_t = rest.trim_start();
        if let Some(r) = rest_t.strip_prefix(['.', ')', '-', ':', '°']) {
            return (r.trim_start(), true);
        }
        if rest.starts_with(char::is_whitespace) && t.starts_with('#') {
            return (rest_t, true);
        }
        return (t, false);
    }
    match t.strip_prefix(['-', '*', '•']) {
        Some(r) => (r.trim_start(), true),
        None => (t, false),
    }
}

/// How a unit is recognised in a line.
struct Matcher {
    idx: usize,
    emoji: String,
    names: Vec<String>,
    id: String,
}

fn matchers(data: &Data) -> Vec<Matcher> {
    let firsts: Vec<String> = data.units.iter().map(|u| fold(u.name.split(' ').next().unwrap_or(""))).collect();
    data.units
        .iter()
        .enumerate()
        .map(|(idx, u)| {
            let mut names = vec![fold(&u.name)];
            // The first word alone ("Géant", "Hydre", "Pilleur") when no other unit shares it.
            let first = &firsts[idx];
            if first.chars().count() >= 4 && first != &names[0] && firsts.iter().filter(|f| *f == first).count() == 1 {
                names.push(first.clone());
            }
            Matcher { idx, emoji: u.emoji.replace('\u{fe0f}', ""), names, id: u.id.clone() }
        })
        .collect()
}

/// Whether `word` appears in `text` as a whole word.
fn has_word(text: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let letter = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !letter(before) && !letter(after) {
            return true;
        }
        from = start + word.len().max(1);
    }
    false
}

enum LineKind {
    /// Not a unit line.
    Other,
    Unit(Entry, Option<u64>),
    Error(String),
    /// Looks like a unit line but names no unit, and isn't numbered: a warning.
    Ignored(String),
}

/// Reads one unit line: "2. ⭐🪖 Hoplite · 2/4/4", "Hoplite 2/4/4 fusionné", "boar 1/4/0"…
fn unit_line(data: &Data, ms: &[Matcher], line: &str, rank: usize) -> LineKind {
    let (body, enumerated) = strip_enumerator(line);
    let folded = fold(body);
    let star = body.contains('⭐') || body.contains('★') || folded.contains("fusionn");
    let triple = find_triple(body);
    // The unit is named before its stats.
    let ident_end = triple.map_or(body.len(), |t| t.0);
    let ident = &body[..ident_end];
    let ident_folded = fold(ident);

    let by_emoji = ms.iter().filter(|m| ident.contains(m.emoji.as_str())).max_by_key(|m| m.emoji.len());
    let by_name = ms
        .iter()
        .filter_map(|m| m.names.iter().filter(|n| has_word(&ident_folded, n)).map(|n| n.len()).max().map(|l| (m, l)))
        .max_by_key(|&(_, l)| l)
        .map(|(m, _)| m);
    let by_id = ms.iter().find(|m| has_word(&ident_folded, &m.id));
    let unit = match (by_emoji, by_name) {
        (Some(e), Some(n)) if e.idx != n.idx => {
            let (de, dn) = (&data.units[e.idx], &data.units[n.idx]);
            return LineKind::Error(format!(
                "Unité {rank} : l'emoji {} ({}) ne correspond pas au nom « {} »",
                de.emoji, de.name, dn.name
            ));
        }
        (Some(e), _) => Some(e),
        (None, Some(n)) => Some(n),
        (None, None) => by_id,
    };
    let Some(m) = unit else {
        let shown = ident.trim().trim_end_matches(['·', '|', '(', ':']).trim();
        let shown = if shown.is_empty() { body.trim() } else { shown };
        if enumerated {
            return LineKind::Error(format!("Unité {rank} : unité inconnue « {shown} »"));
        }
        if triple.is_some() {
            return LineKind::Ignored(format!("Ligne ignorée, aucune unité reconnue : « {} »", line.trim()));
        }
        return LineKind::Other;
    };
    let d = &data.units[m.idx];
    let Some((_, _, [atk, hp, def])) = triple else {
        return LineKind::Error(format!(
            "Unité {rank} ({}) : statistiques illisibles, attendu ATQ/PV/DÉF (par exemple {}/{}/{})",
            d.name, d.atk, d.hp, d.def
        ));
    };
    if [atk, hp, def].iter().any(|&v| v > MAX_STAT as u64) {
        return LineKind::Error(format!("Unité {rank} ({}) : une statistique dépasse {MAX_STAT}", d.name));
    }
    if hp == 0 {
        return LineKind::Error(format!("Unité {rank} ({}) : 0❤, une unité doit avoir au moins 1❤", d.name));
    }
    let entry = Entry { id: d.id.clone(), star, atk: atk as i32, hp: hp as i32, def: def as i32, deaths: 0 };
    LineKind::Unit(entry, find_offering(&fold(&body[ident_end..])).or_else(|| find_offering(&folded)))
}

/// Reads the header fields found in a line: name, 🏆, ✨, defeats and wins in a row.
fn header_fields(line: &str, c: &mut Composition) {
    let folded = fold(line);
    if let Some(at) = line.find("omposition") {
        let mut rest = line[at + "omposition".len()..].trim_start();
        for prefix in ["de ", "De ", "d'", "d’", "D'", ":"] {
            if let Some(r) = rest.strip_prefix(prefix) {
                rest = r.trim_start();
                break;
            }
        }
        let end = ['·', '|', '🏆', '✨', '💔', '🔥', '🔻', '🔺']
            .iter()
            .filter_map(|&ch| rest.find(ch))
            .chain(["série", "Série", "serie", "Serie"].iter().filter_map(|w| rest.find(w)))
            .min()
            .unwrap_or(rest.len());
        let name = clean_name(&rest[..end]);
        if !name.is_empty() {
            c.name = name;
        }
    }
    if let Some(v) = number_near(&folded, "🏆") {
        c.vp = Some(v);
    }
    if let Some(e) = number_near(&folded, "✨") {
        c.essence = Some(e);
    }
    // 🔥 and 💔 since 2026-11-25; 🔺 and 🔻 in older posts.
    let near = |a: &str, b: &str| number_near(&folded, a).or_else(|| number_near(&folded, b));
    if let Some(w) = near("🔥", "🔺").or_else(|| number_near(&folded, "victoire")) {
        c.wins = Some(w);
    }
    if let Some(v) = near("💔", "🔻") {
        c.streak = Some(v);
    } else if let Some(at) = folded.find("serie") {
        let after = &folded[at + "serie".len()..];
        let after = after.trim_start().strip_prefix("de ").unwrap_or(after);
        c.streak = numbers(after).first().filter(|n| after[..n.0].trim().is_empty()).map(|n| n.2 as i32).or(c.streak);
        if c.streak.is_none() && after.contains("aucune") {
            c.streak = Some(0);
        }
    } else if folded.contains("defaite") {
        c.streak = number_near(&folded, "defaite").or(c.streak);
    }
}

/// Reads a composition. Never fails: everything wrong is in `errors`.
pub fn parse(data: &Data, text: &str) -> Parsed {
    let ms = matchers(data);
    let mut p = Parsed::default();
    let mut offerings: Vec<Option<u64>> = Vec::new();
    for raw in normalize(text).lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let folded = fold(line);
        if let Some(at) = folded.find("donnees ") {
            let hash: String = folded[at + "donnees ".len()..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if hash.len() == 8 {
                p.data_hash = Some(hash);
            }
            continue;
        }
        if folded.starts_with("stats") || line.contains("{d100}") {
            continue;
        }
        let header = folded.contains("omposition") || line.contains('🏆') || line.contains('✨');
        if header {
            header_fields(line, &mut p.compo);
            continue;
        }
        match unit_line(data, &ms, line, p.compo.units.len() + 1) {
            LineKind::Unit(e, offering) => {
                p.compo.units.push(e);
                offerings.push(offering);
            }
            LineKind::Error(msg) => {
                p.errors.push(msg);
                // Keeps the numbering of the following units.
                p.compo.units.push(Entry { id: String::new(), star: false, atk: 0, hp: 0, def: 0, deaths: 0 });
                offerings.push(None);
            }
            LineKind::Ignored(msg) => p.warnings.push(msg),
            LineKind::Other => {
                if folded.contains("serie") || folded.contains("defaite") || folded.contains("victoire") {
                    header_fields(line, &mut p.compo);
                }
            }
        }
    }

    let failed: Vec<bool> = p.compo.units.iter().map(|e| e.id.is_empty()).collect();
    for (k, e) in p.compo.units.iter_mut().enumerate() {
        if failed[k] {
            continue;
        }
        let rank = k + 1;
        let idx = data.unit_index(&e.id).unwrap();
        let d = &data.units[idx];
        let needed = offering_needed(data, idx);
        match (needed, offerings[k]) {
            (Some(n), Some(v)) => e.deaths = (v as u32).min(n),
            (Some(n), None) => p.warnings.push(format!("Unité {rank} ({}) : Offrande absente, comptée 0/{n}", d.name)),
            (None, Some(_)) => p.warnings.push(format!("Unité {rank} ({}) : Offrande ignorée, cette unité n'a pas de quête", d.name)),
            (None, None) => {}
        }
        if e.star && d.tier as usize >= data.by_tier.len() && data.rules.top_merge_wins {
            p.errors.push(format!(
                "Unité {rank} ({}) : pas de ⭐ au rang {} (fusionner trois rangs {} est une Ascension)",
                d.name, d.tier, d.tier
            ));
            continue;
        }
        let (ba, bh, bd) = base_stats(data, idx, e.star);
        let star = if e.star { "⭐" } else { "" };
        if e.atk < ba || e.hp < bh || e.def < bd {
            p.warnings.push(format!(
                "Unité {rank} ({star}{}) : {}/{}/{} est en dessous des stats de base {ba}/{bh}/{bd}",
                d.name, e.atk, e.hp, e.def
            ));
        }
        let (sa, sh, sd) = base_stats(data, idx, true);
        if !e.star && power(data, idx, true) > 1 && e.atk >= sa && e.hp >= sh && e.def >= sd {
            p.warnings.push(format!(
                "Unité {rank} ({}) : {}/{}/{} correspond à une unité ⭐ ({sa}/{sh}/{sd}) : ⭐ oublié ?",
                d.name, e.atk, e.hp, e.def
            ));
        }
    }
    p.compo.units.retain(|e| !e.id.is_empty());

    let n = failed.len();
    if n == 0 {
        p.errors.push("Aucune unité trouvée : une ligne par unité, par exemple « 1. 🐗 Sanglier · 1/4/0 »".to_string());
    } else if n > data.rules.board_size {
        p.errors.push(format!("{n} unités : le terrain n'en aligne que {} au maximum", data.rules.board_size));
    }
    if p.compo.vp.is_none() {
        p.warnings.push("🏆 absents : comptés 0".to_string());
    }
    if p.compo.streak.is_none() {
        p.warnings.push("Défaites d'affilée absentes : comptées 0".to_string());
    }
    if p.compo.wins.is_none() {
        p.warnings.push("Victoires d'affilée absentes : comptées 0".to_string());
    }
    if p.compo.essence.is_none() {
        p.warnings.push("✨ absente : le bilan ne pourra pas donner l'essence restante".to_string());
    }
    if let Some(h) = p.data_hash.as_ref().filter(|h| **h != data.hash_label()) {
        p.warnings.push(format!(
            "Composition écrite avec d'autres données ({h}, ici {}) : vérifiez les stats",
            data.hash_label()
        ));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Data {
        Data::from_strs(include_str!("../data/units.toml"), include_str!("../data/rules.toml")).unwrap()
    }

    fn entry(data: &Data, id: &str, star: bool, deaths: u32) -> Entry {
        let idx = data.unit_index(id).unwrap();
        let (atk, hp, def) = base_stats(data, idx, star);
        Entry { id: id.to_string(), star, atk: atk + 1, hp, def, deaths }
    }

    #[test]
    fn round_trip_every_unit() {
        let data = data();
        for u in &data.units {
            let idx = data.unit_index(&u.id).unwrap();
            let stars: &[bool] = if (u.tier as usize) < data.by_tier.len() { &[false, true] } else { &[false] };
            for &star in stars {
                let deaths: &[u32] = if offering_needed(&data, idx).is_some() { &[0, 3, 5] } else { &[0] };
                for &d in deaths {
                    let c = Composition {
                        name: "Alice".into(),
                        vp: Some(3),
                        essence: Some(18),
                        streak: Some(2),
                        wins: Some(1),
                        units: vec![entry(&data, &u.id, star, d), entry(&data, "boar", false, 0)],
                    };
                    let text = format(&data, &c);
                    let p = parse(&data, &text);
                    assert!(p.errors.is_empty(), "{text}\n{:?}", p.errors);
                    assert_eq!(p.compo, c, "{text}");
                    assert_eq!(p.data_hash, None);
                }
            }
        }
    }

    #[test]
    fn lenient() {
        let data = data();
        let text = "[quote][b]Composition de Bob[/b] · 🏆 2 · ✨ 20 · série de 1 défaite\r\n\
                    1) Sanglier 1/5/0\r\n\
                    2) Hoplite fusionné 2/4/4\r\n\
                    3) heidrun · 3 / 4 / 0\r\n\
                    4. \u{26b0}\u{fe0f} ANKOU · 1/3/0 · offrande 2/5[/quote]";
        let p = parse(&data, text);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        let c = &p.compo;
        assert_eq!(c.name, "Bob");
        assert_eq!((c.vp, c.essence, c.streak), (Some(2), Some(20), Some(1)));
        let ids: Vec<&str> = c.units.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["boar", "hoplite", "heidrun", "ankou"]);
        assert!(c.units[1].star);
        assert_eq!(c.units[3].deaths, 2);
        // No emoji, no accents, ★.
        let p = parse(&data, "🏆 0 · ✨ 25 · série de 0 défaite\n1. ★Erinye · 6/6/2\n2. Geant du givre · 2/3/0\n3. 🏺 Embaumeur · 2/4/1");
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(p.compo.units.len(), 3);
        assert!(p.compo.units[0].star);
        assert_eq!(p.compo.units[1].id, "frost_giant");
    }

    #[test]
    fn streak_icons_old_and_new() {
        let data = data();
        for head in [
            "[b]⚔ Composition de Bob[/b] · 🏆 3 · ✨ 18 · 💔 2 défaites d'affilée · 🔥 4 victoires d'affilée",
            "[b]⚔ Composition de Bob[/b] · 🏆 3 · ✨ 18 · 🔻 2 défaites d'affilée · 🔺 4 victoires d'affilée",
        ] {
            let p = parse(&data, &format!("{head}\n1. 🔥 Bénou · 6/6/0\n2. 🐗 Sanglier · 1/4/0"));
            assert!(p.errors.is_empty(), "{:?}", p.errors);
            assert_eq!((p.compo.name.as_str(), p.compo.streak, p.compo.wins), ("Bob", Some(2), Some(4)), "{head}");
            assert_eq!(p.compo.units[0].id, "bennu");
        }
        // Written back with the new icons.
        let p = parse(&data, "Composition de Bob · 🏆 0 · ✨ 25 · 🔻 1 défaite d'affilée · 🔺 0 victoire d'affilée\n1. Sanglier · 1/4/0");
        let text = format(&data, &p.compo);
        assert!(text.contains("💔 1 défaite d'affilée · 🔥 0 victoire d'affilée"), "{text}");
    }

    #[test]
    fn errors_and_warnings() {
        let data = data();
        let p = parse(&data, "1. 🐗 Hoplite · 1/2/2");
        assert!(p.errors[0].contains("ne correspond pas"), "{:?}", p.errors);
        let p = parse(&data, "1. Gorgone · 1/2/2");
        assert!(p.errors[0].contains("inconnue"), "{:?}", p.errors);
        let p = parse(&data, "Posté le 24/11/2026\n1. Sanglier · 1/4/0");
        assert!(p.errors.is_empty() && p.warnings[0].contains("ignorée"), "{:?}", p.warnings);
        let p = parse(&data, "1. Sanglier · 1/0/0");
        assert!(p.errors[0].contains("0❤"));
        let p = parse(&data, "1. Sanglier · 1/1000/0");
        assert!(p.errors[0].contains("999"));
        let p = parse(&data, "1. Sanglier · un/quatre");
        assert!(p.errors[0].contains("illisibles"));
        let p = parse(&data, "1. ⭐🦑 Cthulhu · 8/12/2");
        assert!(p.errors[0].contains("Ascension"));
        let p = parse(&data, "Sanglier 1/4/0\nSanglier 1/4/0\nSanglier 1/4/0\nSanglier 1/4/0\nSanglier 1/4/0");
        assert!(p.errors[0].contains("5 unités"));
        let p = parse(&data, "1. Hoplite · 1/1/2\n2. Hoplite · 2/4/4");
        assert!(p.errors.is_empty());
        assert!(p.warnings.iter().any(|w| w.contains("en dessous")));
        assert!(p.warnings.iter().any(|w| w.contains("⭐ oublié")));
        assert!(p.warnings.iter().any(|w| w.contains("🏆 absents")));
        assert!(p.warnings.iter().any(|w| w.contains("Défaites d'affilée")));
    }
}
