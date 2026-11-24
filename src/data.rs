//! Static game data: units, keywords and rules, loaded from `data/*.toml`.

use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum Archetype {
    Growth,
    Cycle,
    Gamble,
    Control,
    Underdog,
}

impl Archetype {
    pub const ALL: [Archetype; 5] = [
        Archetype::Growth,
        Archetype::Cycle,
        Archetype::Gamble,
        Archetype::Control,
        Archetype::Underdog,
    ];

    pub fn emoji(self) -> &'static str {
        match self {
            Archetype::Growth => "🌱",
            Archetype::Cycle => "♻️",
            Archetype::Gamble => "🎲",
            Archetype::Control => "🔒",
            Archetype::Underdog => "🐺",
        }
    }
}

impl fmt::Display for Archetype {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Archetype::Growth => "Croissance",
            Archetype::Cycle => "Cycle",
            Archetype::Gamble => "Chance",
            Archetype::Control => "Contrôle",
            Archetype::Underdog => "Revanche",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum Pantheon {
    GrecoRoman,
    Norse,
    Egyptian,
    Gaulish,
    /// Outside the four mythologies (the Great Old Ones).
    Elder,
}

impl fmt::Display for Pantheon {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Pantheon::GrecoRoman => "Gréco-Romain",
            Pantheon::Norse => "Scandinave",
            Pantheon::Egyptian => "Égyptien",
            Pantheon::Gaulish => "Gaulois",
            Pantheon::Elder => "Grands Anciens",
        })
    }
}

/// A behaviour attached to a unit. Triggers are documented in the README.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "kind")]
pub enum Keyword {
    /// End of shop: this unit gains the stats permanently.
    Grow {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
        #[serde(default)]
        def: i32,
    },
    /// End of shop: the ally on its left (or every ally, itself included, with `all`) gains the
    /// stats permanently. With `all` it is shown as "Vigueur collective".
    Bless {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
        #[serde(default)]
        all: bool,
    },
    /// On death: gives its ATK to the ally on its right for this fight.
    Legacy,
    /// Start of fight: gains the stats once per ally sharing its ATK, once per ally sharing its
    /// PV and once per ally sharing its DEF (an ally matching two stats counts twice).
    Phalanx {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
    },
    /// When sold: a random ally gains the stats permanently.
    Escort {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
    },
    /// On death: comes back once per fight with 1 PV.
    Rebirth,
    /// The first time it dies in a fight: it comes back with 1 PV, and so does every ally dead
    /// so far in this fight (or falling at the same time). Counts as a Rebirth for Salvage.
    Resurrect,
    /// When sold: +1 favor per Rebirth it triggered, up to `max`.
    Salvage { max: u32 },
    /// On attack: deals `dmg` to a random unit on either side (not itself), ignoring DEF.
    Chaos { dmg: i32 },
    /// Start of fight: eats a random unit on the field, ally or enemy (not itself), and takes its
    /// stats. The eaten unit is out of the fight (not a death). With Fortune, an ally pick is
    /// a failure and is drawn again once.
    Devour,
    /// On attack: coin flip; heads, the damage of this hit becomes favors for the next round
    /// instead of hurting the target, up to `max` favors per fight (the rest still hurts).
    Senet { max: i32 },
    /// On attack: the enemy at the same position (or the last one) loses `amount` ATK.
    Chill { amount: i32 },
    /// Start of fight: the enemy's next attack is cancelled.
    Petrify,
    /// On attack: takes `amount` ATK from the target ("Fascination").
    Steal { amount: i32 },
    /// Fight lost: gains the stats permanently.
    Grudge {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
        #[serde(default)]
        def: i32,
    },
    /// Fight lost: +favors next round.
    Plunder { favors: i32 },
    /// End of shop, coin flip: heads, becomes a copy of the ally on its left (which can complete
    /// a merge); tails, nothing happens.
    Changeling,
    /// Fight lost: favors added straight to the shop investment.
    Toil { favors: i32 },
    /// Favors are no longer capped, and each income adds 1 per `per` favors saved.
    Hoard { per: i32 },
    /// Fight lost: `essence` less divine essence lost, but never less than `floor`. Doesn't stack.
    Endure {
        essence: i32,
        #[serde(default)]
        floor: i32,
    },
    /// On attack, coin flip: heads, its ATK is doubled for this strike (not for ripostes).
    Hubris,
    /// Enemy attacks must target a unit with Taunt while one is alive (abilities ignore it).
    Taunt,
    /// When attacked by an enemy (not by abilities): deals `dmg` to the attacker, ignoring DEF.
    Thorns { dmg: i32 },
    /// On attack: heals the most wounded living ally (lowest PV, itself included) by the
    /// damage it dealt, up to its max PV.
    Lifelink,
    /// Synergy: every end-of-shop growth effect (Grow, Bless) gives +1 more in each stat it raises.
    Sap,
    /// Synergy: once per fight, the first ally that dies for good comes back with 1 PV.
    Recurrence,
    /// Synergy: every failed coin flip (Hubris, Changeling, Senet, Raise) is flipped again once,
    /// and a Chaos or Devour that lands on an ally is drawn again once.
    Fortune,
    /// Synergy: start of fight, every enemy loses `atk` ATK.
    Grip { atk: i32 },
    /// Quest: once it has died `deaths` times over the game (a ⭐ death counts double), its
    /// player is healed `essence` divine essence, once.
    Offering { deaths: u32, essence: i32 },
    /// On attack: 50 % chance (75 % for a ⭐ unit) to bring back a random dead ally with 1 PV.
    Raise,
    /// Start of each fight: every ally gains the stats per fight lost so far, for that fight
    /// only. The best one on the board applies.
    Unleash {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
    },
    /// A unit it kills (attacking or striking back) is destroyed: it can't come back in this
    /// fight, its death powers don't trigger, and its owner loses it (back to the reserve).
    Annihilate,
    /// On attack: the target loses all its powers for the rest of the fight ("Regard de pierre").
    Silence,
    /// When attacked by an enemy (not by abilities): the attacker loses `atk` ATK for the rest of
    /// the fight, after the blow ("Effroi").
    Dread { atk: i32 },
    /// When it kills a unit (attacking or striking back), it dies too ("Châtiment").
    Vengeance,
    /// Start of fight: itself and its two neighbours gain the stats once per ⭐ ally on its
    /// board, itself included, for this fight ("Bonne étoile").
    LuckyStar {
        #[serde(default)]
        atk: i32,
        #[serde(default)]
        hp: i32,
    },
    /// Each day: `favors` per ⭐ unit on its board, with the income ("Pot d'or").
    PotOfGold { favors: i32 },
}

pub fn bonus(atk: i32, hp: i32, def: i32) -> String {
    let mut parts = Vec::new();
    if atk != 0 {
        parts.push(format!("{atk:+}⚔"));
    }
    if hp != 0 {
        parts.push(format!("{hp:+}❤"));
    }
    if def != 0 {
        parts.push(format!("{def:+}🛡"));
    }
    parts.join(" ")
}

impl Keyword {
    /// The keyword as a unit with keyword multiplier `m` (⭐) applies it: its numbers times `m`.
    pub fn scaled(&self, m: i32) -> Keyword {
        match *self {
            Keyword::Grow { atk, hp, def } => Keyword::Grow { atk: atk * m, hp: hp * m, def: def * m },
            Keyword::Bless { atk, hp, all } => Keyword::Bless { atk: atk * m, hp: hp * m, all },
            Keyword::Phalanx { atk, hp } => Keyword::Phalanx { atk: atk * m, hp: hp * m },
            Keyword::Escort { atk, hp } => Keyword::Escort { atk: atk * m, hp: hp * m },
            Keyword::Salvage { max } => Keyword::Salvage { max: max * m as u32 },
            Keyword::Chaos { dmg } => Keyword::Chaos { dmg: dmg * m },
            Keyword::Senet { max } => Keyword::Senet { max: max * m },
            Keyword::Chill { amount } => Keyword::Chill { amount: amount * m },
            Keyword::Steal { amount } => Keyword::Steal { amount: amount * m },
            Keyword::Grudge { atk, hp, def } => Keyword::Grudge { atk: atk * m, hp: hp * m, def: def * m },
            Keyword::Plunder { favors } => Keyword::Plunder { favors: favors * m },
            Keyword::Toil { favors } => Keyword::Toil { favors: favors * m },
            Keyword::Endure { essence, floor } => Keyword::Endure { essence: essence * m, floor },
            Keyword::Thorns { dmg } => Keyword::Thorns { dmg: dmg * m },
            Keyword::Unleash { atk, hp } => Keyword::Unleash { atk: atk * m, hp: hp * m },
            Keyword::Dread { atk } => Keyword::Dread { atk: atk * m },
            Keyword::LuckyStar { atk, hp } => Keyword::LuckyStar { atk: atk * m, hp: hp * m },
            Keyword::PotOfGold { favors } => Keyword::PotOfGold { favors: favors * m },
            k => k,
        }
    }

    /// When the keyword acts and what it does, in plain French, with the numbers of the rules.
    pub fn explain(&self, rules: &Rules) -> (&'static str, String) {
        self.explain_at(rules, 1)
    }

    /// Same, for a unit with keyword multiplier `m` (2 for a ⭐ unit): the numbers are doubled,
    /// and the keywords whose ⭐ effect isn't a number say what changes.
    pub fn explain_at(&self, rules: &Rules, m: i32) -> (&'static str, String) {
        let (when, mut text) = self.scaled(m).base_explain(rules);
        if m > 1 {
            match *self {
                Keyword::Petrify => {
                    text = format!("Les {m} prochaines attaques du camp adverse sont annulées, quelles que soient les unités qui devaient frapper.")
                }
                Keyword::Legacy => {
                    text = format!("Donne {m} fois son ATQ à l'allié placé juste à sa droite, pour la suite du combat.")
                }
                Keyword::Devour => {
                    text = text.replace("ajoute son ATQ, ses PV et sa DÉF aux siens", &format!("ajoute {m} fois son ATQ, ses PV et sa DÉF aux siens"))
                }
                Keyword::Raise => {
                    text = "3 chances sur 4 de ramener un allié mort dans ce combat, au hasard, avec 1❤. Jamais une unité qui a elle-même Momification.".to_string()
                }
                Keyword::Plunder { .. } => {
                    text = format!(
                        "Chaque jour, s'il est sur le terrain, rapporte +{}🙏 par défaite subie dans la journée.",
                        rules.arena.plunder_per_loss * m
                    )
                }
                Keyword::Hoard { per } => {
                    text = format!(
                        "Plus de plafond de faveurs ({}🙏 sinon), et chaque revenu rapporte +{m}🙏 pour {per}🙏 gardées : idéal pour épargner.",
                        rules.favor_cap
                    )
                }
                Keyword::Offering { .. } => text += " Fusionnée, chacune de ses morts compte double.",
                Keyword::Recurrence => {
                    text = format!("Dans chaque combat, les {m} premiers alliés qui meurent définitivement (lui compris) reviennent avec 1❤. S'ajoute à la synergie ♻️ Éternel retour.")
                }
                _ => {}
            }
        }
        (when, text)
    }

    fn base_explain(&self, rules: &Rules) -> (&'static str, String) {
        let pl = |n: i32| if n.abs() > 1 { "s" } else { "" };
        match *self {
            Keyword::Grow { atk, hp, def } => (
                "Chaque jour",
                format!("Chaque jour, gagne {} en bonus permanent : il grandit tant qu'il reste sur le terrain.", bonus(atk, hp, def)),
            ),
            Keyword::Bless { atk, hp, all: true } => (
                "Chaque jour",
                format!("Chaque jour, tous les alliés du terrain, lui compris, gagnent {} en bonus permanent.", bonus(atk, hp, 0)),
            ),
            Keyword::Bless { atk, hp, all: false } => (
                "Chaque jour",
                format!("Chaque jour, l'allié placé juste à sa gauche gagne {} en bonus permanent : placez à sa gauche l'unité à faire grandir.", bonus(atk, hp, 0)),
            ),
            Keyword::Legacy => (
                "À sa mort",
                "Donne son ATQ à l'allié placé juste à sa droite, pour la suite du combat. À chaque mort, même s'il revient ensuite (Renaissance, Éternel retour…).".to_string(),
            ),
            Keyword::Phalanx { atk, hp } => (
                "Début de combat",
                format!(
                    "Gagne {} pour ce combat par allié qui a la même ATQ que lui, encore {} par allié qui a les mêmes PV, et encore par allié qui a la même DÉF. Recalculé quand un allié meurt ou change.",
                    bonus(atk, hp, 0),
                    bonus(atk, hp, 0)
                ),
            ),
            Keyword::Escort { atk, hp } => (
                "À la vente",
                format!("Quand on la vend, même depuis la réserve, un allié du terrain au hasard gagne {} en bonus permanent.", bonus(atk, hp, 0)),
            ),
            Keyword::Rebirth => (
                "À sa mort",
                "La première fois qu'il meurt dans un combat, il revient avec 1❤ et continue de se battre. Une seule fois par combat, même ⭐.".to_string(),
            ),
            Keyword::Resurrect => (
                "À sa mort",
                "La première fois qu'il meurt dans un combat, il revient avec 1❤ et ramène avec lui tous les alliés déjà morts dans ce combat, avec 1❤, à leur place. Une seule fois par combat.".to_string(),
            ),
            Keyword::Salvage { max } => (
                "À la vente",
                format!("Rapporte +1🙏 pour chaque retour à la vie déclenché pendant la partie, jusqu'à {max}🙏."),
            ),
            Keyword::Chaos { dmg } => (
                "Après chaque attaque",
                format!("Inflige {dmg} dégât{} qui ignore{} la DÉF à une unité vivante au hasard, ennemie ou alliée (jamais lui-même).", pl(dmg), if dmg > 1 { "nt" } else { "" }),
            ),
            Keyword::Devour => (
                "Début de combat",
                "Dévore une unité au hasard sur tout le terrain, ennemie ou alliée, et ajoute son ATQ, ses PV et sa DÉF aux siens pour ce combat. L'unité dévorée sort du combat (ce n'est pas une mort) et revient ensuite à son propriétaire.".to_string(),
            ),
            Keyword::Senet { max } => (
                "À chaque attaque",
                format!("Pile ou face. Pile : les dégâts du coup ne blessent pas la cible, ils deviennent des faveurs, reçues aussitôt. Au plus {max}🙏 par jour ; ensuite, elle frappe normalement."),
            ),
            Keyword::Chill { amount } => (
                "Avant chaque attaque",
                format!("L'ennemi placé à la même position que lui (ou le dernier de la ligne, s'il n'y en a pas) perd {amount}⚔ pour ce combat. Placez-le face à la plus grosse menace."),
            ),
            Keyword::Petrify => (
                "Début de combat",
                "La prochaine attaque du camp adverse est annulée, quelle que soit l'unité qui devait frapper.".to_string(),
            ),
            Keyword::Steal { amount } => (
                "Après chaque attaque",
                format!("Prend {amount}⚔ à sa cible et les ajoute aux siennes, pour le reste du combat : plus elle frappe, plus l'ennemi faiblit."),
            ),
            Keyword::Grudge { atk, hp, def } => (
                "Après une défaite",
                format!("Chaque combat perdu lui donne {} en bonus permanent : plus vous perdez, plus il devient fort.", bonus(atk, hp, def)),
            ),
            Keyword::Plunder { .. } => (
                "Chaque jour",
                format!(
                    "Chaque jour, s'il est sur le terrain, rapporte +{}🙏 par défaite subie dans la journée.",
                    rules.arena.plunder_per_loss
                ),
            ),
            Keyword::Changeling => (
                "Chaque jour",
                "Chaque jour, pile ou face. Pile : il devient une copie de l'allié placé à sa gauche, s'il en reste un exemplaire dans la réserve commune (la copie peut compléter une fusion). Face : rien.".to_string(),
            ),
            Keyword::Toil { favors } => (
                "Après une défaite",
                format!("Verse +{favors}🙏 dans l'investissement de la boutique."),
            ),
            Keyword::Hoard { per } => (
                "En permanence",
                format!("Plus de plafond de faveurs ({}🙏 sinon), et chaque revenu rapporte +1🙏 pour {per}🙏 gardées : idéal pour épargner.", rules.favor_cap),
            ),
            Keyword::Endure { essence, floor } => (
                "Défaite",
                format!("Une défaite coûte {essence}✨ de moins, sans descendre sous {floor}✨."),
            ),
            Keyword::Hubris => (
                "À chaque attaque",
                "Pile ou face. Pile : son ATQ est doublée pour ce coup (pas pour ses ripostes).".to_string(),
            ),
            Keyword::Taunt => (
                "En combat",
                "Tant qu'il est vivant, les ennemis doivent l'attaquer, lui ou une autre unité avec Provocation : il protège les unités fragiles. Les pouvoirs (Chaos, Gel…) l'ignorent.".to_string(),
            ),
            Keyword::Thorns { dmg } => (
                "Quand on l'attaque",
                format!("L'attaquant subit {dmg} dégât{} qui ignore{} la DÉF, même si cette unité tombe. Les pouvoirs ne déclenchent pas Représailles.", pl(dmg), if dmg > 1 { "nt" } else { "" }),
            ),
            Keyword::Lifelink => (
                "Après chaque attaque",
                "Soigne l'allié vivant le plus blessé (elle-même comprise) d'autant de PV que les dégâts qu'elle vient d'infliger, sans dépasser ses PV de départ.".to_string(),
            ),
            Keyword::Sap => (
                "Synergie",
                "Chaque Vigueur et Bénédiction donne +1 de plus dans chaque caractéristique qu'elle augmente.".to_string(),
            ),
            Keyword::Recurrence => (
                "En combat",
                "Une fois par combat, le premier allié qui meurt définitivement (lui compris) revient avec 1❤. S'ajoute à la synergie ♻️ Éternel retour.".to_string(),
            ),
            Keyword::Fortune => (
                "Synergie",
                "Chaque pile ou face raté est retenté une fois ; un Chaos ou une Dévoration qui tombe sur un allié est retiré une fois.".to_string(),
            ),
            Keyword::Grip { atk } => ("Synergie", format!("Au début du combat, chaque ennemi perd {atk}⚔.")),
            Keyword::Offering { deaths, essence } => (
                "Quête",
                format!(
                    "Quand l'unité est morte {deaths} fois au total sur la partie, son joueur récupère {essence}✨, une seule fois (sans dépasser {}✨). Une mort d'unité ⭐ compte double. C'est le seul moyen de regagner de l'essence.",
                    rules.starting_essence
                ),
            ),
            Keyword::Raise => (
                "À chaque attaque",
                "1 chance sur 2 (3 sur 4 si ⭐) de ramener un allié mort dans ce combat, au hasard, avec 1❤. Jamais une unité qui a elle-même Momification.".to_string(),
            ),
            Keyword::Annihilate => (
                "Quand il tue",
                format!(
                    "Une unité qu'il tue, en attaquant ou en ripostant, est anéantie : elle ne revient pas dans ce combat (Renaissance, Éternel retour, Résurrection, Momification sans effet), ses pouvoirs de mort ne se déclenchent pas, et son propriétaire la perd pour de bon : elle retourne dans la réserve commune ({} exemplaires pour une unité ⭐).",
                    rules.merge_copies
                ),
            ),
            Keyword::Silence => (
                "À chaque attaque",
                "Sa cible perd tous ses pouvoirs pour le reste du combat (Provocation, Représailles, Renaissance…) et garde ses stats. Ses Représailles ne répondent déjà pas à ce coup.".to_string(),
            ),
            Keyword::Dread { atk } => (
                "Quand on l'attaque",
                format!("L'attaquant perd {atk}⚔ pour le reste du combat, après son coup. Les pouvoirs ne déclenchent pas Effroi."),
            ),
            Keyword::Vengeance => (
                "Quand elle tue",
                "Quand elle tue une unité, en attaquant ou en ripostant, elle meurt aussi : sa vengeance est accomplie.".to_string(),
            ),
            Keyword::LuckyStar { atk, hp } => (
                "Début de combat",
                format!("Lui et ses deux voisins gagnent {} pour ce combat par unité ⭐ sur le terrain de son joueur, lui compris.", bonus(atk, hp, 0)),
            ),
            Keyword::PotOfGold { favors } => (
                "Chaque jour",
                format!(
                    "Chaque jour, rapporte +{favors}🙏 par unité ⭐ sur le terrain de son joueur, avec le revenu (plafond de {}🙏).",
                    rules.favor_cap
                ),
            ),
            Keyword::Unleash { atk, hp } => (
                "Début de combat",
                format!(
                    "Chaque allié gagne {} par défaite d'affilée de son joueur (au plus {}), pour ce combat seulement. Avec plusieurs Cthulhu, seul le plus fort compte.",
                    bonus(atk, hp, 0),
                    rules.arena.unleash_max
                ),
            ),
        }
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Keyword::Grow { atk, hp, def } => write!(f, "Vigueur {}", bonus(atk, hp, def)),
            Keyword::Bless { atk, hp, all } => {
                let name = if all { "Vigueur collective" } else { "Bénédiction" };
                write!(f, "{name} {}", bonus(atk, hp, 0))
            }
            Keyword::Legacy => write!(f, "Héritage"),
            Keyword::Phalanx { atk, hp } => write!(f, "Phalange {}/caractéristique commune", bonus(atk, hp, 0)),
            Keyword::Escort { atk, hp } => write!(f, "Escorte {}", bonus(atk, hp, 0)),
            Keyword::Rebirth => write!(f, "Renaissance"),
            Keyword::Resurrect => write!(f, "Résurrection"),
            Keyword::Salvage { max } => write!(f, "Récupération (max {max})"),
            Keyword::Chaos { dmg } => write!(f, "Chaos {dmg} par attaque"),
            Keyword::Devour => write!(f, "Dévoration"),
            Keyword::Senet { max } => write!(f, "Senet ({max}🙏 max par combat)"),
            Keyword::Chill { amount } => write!(f, "Gel {amount} par attaque"),
            Keyword::Petrify => write!(f, "Pétrification"),
            Keyword::Steal { amount } => write!(f, "Fascination {amount}"),
            Keyword::Grudge { atk, hp, def } => write!(f, "Rancune {}", bonus(atk, hp, def)),
            Keyword::Plunder { favors } => write!(f, "Pillage +{favors}🙏"),
            Keyword::Changeling => write!(f, "Changelin"),
            Keyword::Toil { favors } => write!(f, "Labeur +{favors}🙏 investie"),
            Keyword::Hoard { per } => write!(f, "Magot (sans plafond, +1🙏 par {per} gardées)"),
            Keyword::Endure { essence, floor } => write!(f, "Endurance (−{essence}✨ perdue, min {floor})"),
            Keyword::Hubris => write!(f, "Démesure"),
            Keyword::Taunt => write!(f, "Provocation"),
            Keyword::Lifelink => write!(f, "Hydromel"),
            Keyword::Sap => write!(f, "Sève (+1 aux Vigueur et Bénédiction)"),
            Keyword::Recurrence => write!(f, "Éternel retour (le premier allié mort revient avec 1❤)"),
            Keyword::Fortune => write!(f, "Fortune (chaque tirage raté, ou tombé sur un allié, est retenté une fois)"),
            Keyword::Grip { atk } => write!(f, "Emprise (chaque ennemi −{atk}⚔ au début du combat)"),
            Keyword::Thorns { dmg } => write!(f, "Représailles {dmg}"),
            Keyword::Offering { deaths, essence } => write!(f, "Offrande ({deaths} morts → +{essence}✨)"),
            Keyword::Raise => write!(f, "Momification"),
            Keyword::Unleash { atk, hp } => {
                write!(f, "Déchaînement {}/défaite à toute l'armée en combat", bonus(atk, hp, 0))
            }
            Keyword::Annihilate => write!(f, "Néant"),
            Keyword::Silence => write!(f, "Regard de pierre"),
            Keyword::Dread { atk } => write!(f, "Effroi −{atk}⚔"),
            Keyword::Vengeance => write!(f, "Châtiment"),
            Keyword::LuckyStar { atk, hp } => write!(f, "Bonne étoile {}/⭐", bonus(atk, hp, 0)),
            Keyword::PotOfGold { favors } => write!(f, "Pot d'or +{favors}🙏/⭐"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnitDef {
    pub id: String,
    pub name: String,
    pub emoji: String,
    pub pantheon: Pantheon,
    pub archetype: Archetype,
    /// Second archetype of a dual unit: it counts for both (synergies, bots).
    #[serde(default)]
    pub archetype2: Option<Archetype>,
    pub tier: u8,
    pub atk: i32,
    pub hp: i32,
    pub def: i32,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
}

impl UnitDef {
    /// Belongs to this archetype, as its main or second one.
    pub fn is(&self, a: Archetype) -> bool {
        self.archetype == a || self.archetype2 == Some(a)
    }

    /// "🌱 Croissance", or "🌱 Croissance / ♻️ Cycle" for a dual unit.
    pub fn archetype_label(&self) -> String {
        let one = |a: Archetype| format!("{} {}", a.emoji(), a);
        match self.archetype2 {
            Some(b) => format!("{} / {}", one(self.archetype), one(b)),
            None => one(self.archetype),
        }
    }

    pub fn has(&self, pred: impl Fn(&Keyword) -> bool) -> bool {
        self.keywords.iter().any(pred)
    }
}

/// A bonus for fielding enough units of one archetype.
#[derive(Debug, Clone, Deserialize)]
pub struct Synergy {
    pub archetype: Archetype,
    pub count: usize,
    pub keyword: Keyword,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rules {
    pub players: usize,
    pub rounds: usize,
    pub board_size: usize,
    pub reserve_size: usize,
    pub base_income: i32,
    pub favor_cap: i32,
    pub sell_value: i32,
    pub reroll_cost: i32,
    pub tier_cost: Vec<i32>,
    pub pool_per_player: Vec<f64>,
    pub merge_copies: usize,
    pub merge_power: Vec<i32>,
    pub merge_reward: bool,
    pub top_merge_wins: bool,
    pub shop_levels: Vec<Vec<u8>>,
    pub upgrade_threshold: Vec<i32>,
    pub starting_essence: i32,
    pub loss_per_tier: i32,
    pub merged_factor: i32,
    pub draw_loss_per_round: i32,
    pub min_damage: i32,
    pub max_fight_steps: usize,
    #[serde(default)]
    pub synergy: Vec<Synergy>,
    /// Arena mode (continuous play, forum shop).
    #[serde(default)]
    pub arena: ArenaRules,
}

/// Arena mode: days instead of rounds, the Bazar (a forum thread whose pages are shops), and
/// fights between two players who both agree.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ArenaRules {
    pub days: usize,
    /// Units in each level's shop on a Bazar page.
    pub bazar_size: usize,
    /// Posts per forum page: a full page draws a new shop.
    pub page_posts: usize,
    /// Favors each Bazar post costs; a unit costs its tier price minus this.
    pub post_cost: i32,
    /// Chance (%) of each tier for one slot, per shop level.
    pub bazar_odds: Vec<Vec<u32>>,
    pub max_fights_per_day: u32,
    /// Favors for the winner of a fight, on top of the victory point.
    pub win_favors: i32,
    /// Essence each player loses on a draw.
    pub draw_loss: i32,
    /// Every `comeback_losses` defeats in a row pay the comeback bonus (`comeback_favors`).
    pub comeback_losses: u32,
    pub comeback_favors: i32,
    /// From the `win_streak_from`th win in a row, each win pays `win_streak_favors` more.
    pub win_streak_from: u32,
    pub win_streak_favors: i32,
    pub ascension_vp: i32,
    /// Pillage: favors per defeat of the day, paid at the daily tick.
    pub plunder_per_loss: i32,
    /// Déchaînement counts the current losing streak, up to this.
    pub unleash_max: i32,
    /// Victory points lost by a player who didn't fight during a day.
    pub idle_vp_loss: i32,
    /// The challenged player sets its line-up after seeing the challenger's.
    pub counter_pick: bool,
}

impl Default for ArenaRules {
    fn default() -> ArenaRules {
        ArenaRules {
            days: 7,
            bazar_size: 7,
            page_posts: 10,
            post_cost: 1,
            bazar_odds: vec![vec![80, 20, 0], vec![55, 45, 0], vec![30, 65, 5], vec![15, 75, 10]],
            max_fights_per_day: 6,
            win_favors: 1,
            draw_loss: 1,
            comeback_losses: 3,
            comeback_favors: 5,
            win_streak_from: 3,
            win_streak_favors: 1,
            ascension_vp: 5,
            plunder_per_loss: 1,
            unleash_max: 5,
            idle_vp_loss: 1,
            counter_pick: true,
        }
    }
}

#[derive(Deserialize)]
struct UnitsFile {
    unit: Vec<UnitDef>,
}

pub struct Data {
    pub units: Vec<UnitDef>,
    pub rules: Rules,
    /// Unit indices for each tier (index 0 = tier 1).
    pub by_tier: Vec<Vec<usize>>,
    /// FNV-1a 64 of both files (line endings normalised): tells which data a fight was run with.
    pub fingerprint: u64,
}

/// FNV-1a 64 of `units\0rules`, with CRLF read as LF and without a leading BOM, so that a
/// checkout with Windows line endings gives the same fingerprint.
pub fn fingerprint(units: &str, rules: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let norm = |s: &str| s.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let text = format!("{}\0{}", norm(units), norm(rules));
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Data {
    pub fn load(dir: &Path) -> Result<Data, String> {
        let read = |name: &str| {
            let path = dir.join(name);
            fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
        };
        Data::from_strs(&read("units.toml")?, &read("rules.toml")?)
    }

    /// Builds the data from the contents of units.toml and rules.toml.
    pub fn from_strs(units_text: &str, rules_text: &str) -> Result<Data, String> {
        let units: UnitsFile = toml::from_str(units_text).map_err(|e| format!("units.toml: {e}"))?;
        let rules: Rules = toml::from_str(rules_text).map_err(|e| format!("rules.toml: {e}"))?;
        let fingerprint = fingerprint(units_text, rules_text);

        let tiers = rules.tier_cost.len();
        let mut by_tier = vec![Vec::new(); tiers];
        for (i, u) in units.unit.iter().enumerate() {
            if u.tier == 0 || u.tier as usize > tiers {
                return Err(format!("unité {} : le rang {} n'a pas de prix dans les règles", u.id, u.tier));
            }
            if units.unit[..i].iter().any(|o| o.id == u.id) {
                return Err(format!("identifiant d'unité en double : {}", u.id));
            }
            by_tier[u.tier as usize - 1].push(i);
        }
        if rules.shop_levels.len() != rules.upgrade_threshold.len() + 1 {
            return Err("règles : upgrade_threshold doit avoir une entrée par niveau de boutique après le premier".into());
        }
        for level in &rules.shop_levels {
            for &t in level {
                if t == 0 || t as usize > tiers || by_tier[t as usize - 1].is_empty() {
                    return Err(format!("règles : aucun emplacement de boutique de rang {t} n'a d'unités"));
                }
            }
        }
        if rules.merge_power.len() != tiers {
            return Err("règles : merge_power doit avoir une entrée par rang".into());
        }
        if rules.pool_per_player.len() != tiers {
            return Err("règles : pool_per_player doit avoir une entrée par rang".into());
        }
        Ok(Data { units: units.unit, rules, by_tier, fingerprint })
    }

    /// The fingerprint as shown to players: 8 hex digits.
    pub fn hash_label(&self) -> String {
        format!("{:08x}", (self.fingerprint ^ (self.fingerprint >> 32)) as u32)
    }

    /// A unit by its id ("boar").
    pub fn unit_index(&self, id: &str) -> Option<usize> {
        self.units.iter().position(|u| u.id == id)
    }

    pub fn cost(&self, id: usize) -> i32 {
        self.rules.tier_cost[self.units[id].tier as usize - 1]
    }

    /// Copies of each unit of `tier` in the shared pool, for the lobby size.
    pub fn pool_copies(&self, tier: u8) -> u32 {
        self.pool_copies_for(tier, self.rules.players as u32)
    }

    /// Copies of each unit of `tier` in the shared pool for `players` registered players
    /// (rounded, never fewer than a merge needs): the reserve grows as players join.
    pub fn pool_copies_for(&self, tier: u8, players: u32) -> u32 {
        let r = &self.rules;
        let copies = (r.pool_per_player[tier as usize - 1] * players as f64).round() as u32;
        copies.max(r.merge_copies as u32)
    }

    /// Essence a defeat costs, given the winner's survivors as (unit id, keyword multiplier).
    pub fn defeat_loss(&self, survivors: &[(usize, i32)]) -> i32 {
        let r = &self.rules;
        survivors
            .iter()
            .map(|&(id, mult)| self.units[id].tier as i32 * r.loss_per_tier * if mult > 1 { r.merged_factor } else { 1 })
            .sum()
    }

    /// Essence a draw costs each player; `round` counts from 0.
    pub fn draw_loss(&self, round: usize) -> i32 {
        self.rules.draw_loss_per_round * (round as i32 + 1)
    }
}
