//! Forum (Twino) layout of a fight: line-ups in the open, the fight and its result in collapse
//! blocks.

/// The lines of a fight post. `start` is the line-up before the fight (one line per side),
/// `prelude` the lines shown under it (Déchaînement, synergies), `log` the traced fight (its
/// first two lines repeat the line-up and are skipped), `verdict` the outcome line and `end`
/// the line-up of the survivors.
pub fn fight_post(title: &str, start: &str, prelude: &[String], log: &[String], verdict: &str, end: &str) -> Vec<String> {
    let mut out = vec![format!("[b]{title}[/b]"), start.to_string()];
    out.extend(prelude.iter().cloned());
    out.push(String::new());
    // An empty line of the log opens a turn: a rule between turns.
    let log: Vec<String> = log
        .iter()
        .skip(2)
        .map(|line| if line.trim().is_empty() { "{hr}".to_string() } else { line.trim_start().to_string() })
        .collect();
    out.extend(collapse("Déroulé du combat", &log));
    out.push(String::new());
    out.extend(collapse("Résultat", &[verdict.to_string(), end.to_string()]));
    out.push("{hr}".to_string());
    out
}

/// A "[collapse=title]" block. MyHordes shows a line break right after the opening tag or
/// before the closing one, so the tags hug the first and last lines.
pub fn collapse(title: &str, lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = lines.to_vec();
    if out.is_empty() {
        out.push(String::new());
    }
    out[0] = format!("[collapse={title}]{}", out[0]);
    let last = out.len() - 1;
    out[last].push_str("[/collapse]");
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
                // "[collapse=Title]text": the title on its own line, then the text.
                if let Some(rest) = l.trim_start().strip_prefix("[collapse=") {
                    if let Some(end) = rest.find(']') {
                        out.push(rest[..end].to_string());
                        l = rest[end + 1..].to_string();
                    }
                }
                l = l.replace("[/collapse]", "");
                for tag in ["[b]", "[/b]", "[i]", "[/i]", "[big]", "[/big]", "[spoiler]", "[/spoiler]"] {
                    l = l.replace(tag, "");
                }
                out.push(l);
            }
        }
    }
    out.join("\n")
}
