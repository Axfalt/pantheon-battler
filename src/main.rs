//! Pantheon battler: an auto battler played on the MyHordes forum.
//! This binary is the balance simulator with bot players, plus the Arena challenge tools.

use clap::{Parser, Subcommand};
use pantheon::bot::{self, Strategy};
use pantheon::data::Data;
use pantheon::game::{self, GameResult};
use pantheon::state::GameRng;
use pantheon::{api, arena, challenge, report};
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Pantheon battler : moteur de règles et simulateur d'équilibrage")]
struct Cli {
    /// Dossier contenant units.toml et rules.toml
    #[arg(long, default_value = "data")]
    data: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Affiche la liste des unités
    Roster,
    /// Simule de nombreuses parties entre bots et donne les chiffres d'équilibrage
    Games {
        #[arg(short, long, default_value_t = 20000)]
        n: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Stratégie imposée aux `count` premières places (ex. Croissance-moyen) ; les autres restent aléatoires
        #[arg(long)]
        fixed: Option<String>,
        #[arg(long, default_value_t = 0)]
        count: usize,
    },
    /// Simule le mode Arène (jeu continu, Bazar, défis acceptés) et donne les chiffres d'équilibrage
    Arena {
        #[arg(short, long, default_value_t = 2000)]
        n: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Duels entre chaque paire d'unités
    Duels {
        /// Seulement les unités de ce rang
        #[arg(long)]
        tier: Option<u8>,
        #[arg(short, long, default_value_t = 200)]
        n: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Joue une partie et affiche son journal complet
    Sample {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Combats au format forum (Twino) : alignements visibles, déroulé et résultat en spoilers
        #[arg(long)]
        forum: bool,
    },
    /// Joue un défi de l'Arène : deux compositions et les six dés du forum (comme la page web)
    Fight {
        /// Composition de l'attaquant (fichier texte, telle que postée sur le forum)
        #[arg(long)]
        a_file: PathBuf,
        /// Composition du défenseur
        #[arg(long)]
        b_file: PathBuf,
        /// Les six dés : ceux de l'attaquant puis ceux du défenseur (ex. 12,3,77,45,9,100)
        #[arg(long, value_delimiter = ',')]
        dice: Vec<u32>,
        /// Nom de l'attaquant (sinon celui écrit dans sa composition)
        #[arg(long)]
        a_name: Option<String>,
        /// Nom du défenseur
        #[arg(long)]
        b_name: Option<String>,
        /// Texte au format forum (Twino) plutôt qu'en texte brut
        #[arg(long)]
        forum: bool,
    },
    /// API JSON de la page web : une requête par ligne sur l'entrée standard
    #[command(hide = true)]
    Api,
}

fn main() {
    let cli = Cli::parse();
    let data = Data::load(&cli.data).unwrap_or_else(|e| {
        eprintln!("erreur : {e}");
        std::process::exit(1);
    });
    let strats = bot::default_strategies(&data.rules);

    match cli.cmd {
        Cmd::Roster => print!("{}", report::roster(&data)),
        Cmd::Fight { a_file, b_file, dice, a_name, b_name, forum } => {
            let read = |path: &PathBuf| {
                std::fs::read_to_string(path).unwrap_or_else(|e| {
                    eprintln!("erreur : {} : {e}", path.display());
                    std::process::exit(1);
                })
            };
            let (a, b) = (read(&a_file), read(&b_file));
            match challenge::challenge(&data, [&a, &b], [a_name.as_deref(), b_name.as_deref()], &dice) {
                Ok(c) => {
                    for w in &c.warnings {
                        eprintln!("attention : {w}");
                    }
                    println!("{}", if forum { c.forum_text() } else { c.plain_text() });
                }
                Err(errors) => {
                    for e in errors {
                        eprintln!("erreur : {e}");
                    }
                    std::process::exit(1);
                }
            }
        }
        Cmd::Api => {
            let stdin = std::io::stdin();
            let mut out = std::io::stdout().lock();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                writeln!(out, "{}", api::handle(&data, &line)).ok();
                out.flush().ok();
            }
        }
        Cmd::Games { n, seed, fixed, count } => {
            let fixed = fixed.map(|name| {
                strats.iter().position(|s| s.name == name).unwrap_or_else(|| {
                    let names: Vec<&str> = strats.iter().map(|s| s.name.as_str()).collect();
                    eprintln!("erreur : stratégie inconnue {name} ; connues : {}", names.join(", "));
                    std::process::exit(1);
                })
            });
            let results: Vec<GameResult> = (0..n)
                .into_par_iter()
                .map(|g| {
                    let mut rng = game_rng(seed, g as u64);
                    let mut lobby = random_lobby(&data, &strats, &mut rng);
                    if let Some(f) = fixed {
                        lobby.iter_mut().take(count).for_each(|seat| *seat = f);
                    }
                    game::run_game(&data, &strats, &lobby, &mut rng, None, false)
                })
                .collect();
            print!("{}", report::games(&data, &strats, &results));
        }
        Cmd::Arena { n, seed } => {
            // The rush plan (invest before buying) means nothing without shop phases.
            let usable: Vec<usize> = (0..strats.len()).filter(|&i| strats[i].plan != "rush").collect();
            let results: Vec<arena::ArenaResult> = (0..n)
                .into_par_iter()
                .map(|g| {
                    let mut rng = game_rng(seed, g as u64);
                    let seats: Vec<arena::Seat> = (0..data.rules.players)
                        .map(|_| arena::Seat {
                            strat: usable[rng.gen_range(0..usable.len())],
                            activity: arena::Activity::ALL[rng.gen_range(0..3)],
                            attitude: arena::Attitude::ALL[rng.gen_range(0..2)],
                        })
                        .collect();
                    arena::run(&data, &strats, &seats, &mut rng)
                })
                .collect();
            print!("{}", report::arena(&data, &strats, &results));
        }
        Cmd::Duels { tier, n, seed } => print!("{}", report::duels(&data, tier, n, seed)),
        Cmd::Sample { seed, forum } => {
            let mut rng = game_rng(seed, 0);
            let lobby = random_lobby(&data, &strats, &mut rng);
            let mut log = Vec::new();
            game::run_game(&data, &strats, &lobby, &mut rng, Some(&mut log), forum);
            println!("{}", log.join("\n"));
        }
    }
}

fn game_rng(seed: u64, game: u64) -> GameRng {
    GameRng::seed_from_u64(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ game)
}

/// Each seat gets a strategy drawn uniformly at random.
fn random_lobby(data: &Data, strats: &[Strategy], rng: &mut GameRng) -> Vec<usize> {
    (0..data.rules.players).map(|_| rng.gen_range(0..strats.len())).collect()
}
