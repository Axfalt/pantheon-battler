//! Forum (Twino) layout of a fight: line-ups in the open, the fight and its result in spoilers.

/// The lines of a fight post. `start` is the line-up before the fight (one line per side),
/// `prelude` the lines shown under it (Déchaînement, synergies), `log` the traced fight (its
/// first two lines repeat the line-up and are skipped), `verdict` the outcome line and `end`
/// the line-up of the survivors.
pub fn fight_post(title: &str, start: &str, prelude: &[String], log: &[String], verdict: &str, end: &str) -> Vec<String> {
    let mut out = vec![format!("[b]{title}[/b]"), start.to_string()];
    out.extend(prelude.iter().cloned());
    out.push(String::new());
    out.push("[b]Déroulé du combat[/b]".to_string());
    out.push("[spoiler]".to_string());
    out.extend(log.iter().skip(2).map(|line| line.trim_start().to_string()));
    out.push("[/spoiler]\n".to_string());
    out.push("[b]Résultat[/b]".to_string());
    out.push("[spoiler]".to_string());
    out.push(verdict.to_string());
    out.push(end.to_string());
    out.push("[/spoiler]".to_string());
    out.push("{hr}".to_string());
    out
}

/// "de Bob", "d'Alice": French elision before a vowel.
pub fn de(name: &str) -> String {
    let vowel = name.chars().next().is_some_and(|c| "AEIOUYÉÈÊÂÎÔaeiouyéèêâîô".contains(c));
    if vowel { format!("d'{name}") } else { format!("de {name}") }
}

/// A forum post without its markup, for a plain-text copy.
pub fn plain(text: &str) -> String {
    let mut out = Vec::new();
    for line in text.split('\n') {
        match line.trim() {
            "[spoiler]" | "[/spoiler]" => continue,
            "{hr}" => out.push("――――――――――――――――".to_string()),
            _ => {
                let mut l = line.to_string();
                for tag in ["[b]", "[/b]", "[i]", "[/i]", "[big]", "[/big]", "[spoiler]", "[/spoiler]"] {
                    l = l.replace(tag, "");
                }
                out.push(l);
            }
        }
    }
    out.join("\n")
}
