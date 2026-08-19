//! `riposte` — headless driver for the parts of the pipeline that do not need
//! audio hardware. Milestones M0 and M1 of `docs/07-BUILD-PLAN.md`.

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use riposte_core::{CardOrigin, Objection, ObjectionKind};
use riposte_detect::{DetectionGate, Embedder, HashEmbedder};
use riposte_playbook::PlaybookFile;
use riposte_store::Store;

// Palette from docs/05-UX-SPEC.md §2, as truecolor escapes.
const REBUT: &str = "\x1b[38;2;77;163;255m";
const COMPLY: &str = "\x1b[38;2;255;107;77m";
const DISQUALIFY: &str = "\x1b[38;2;185;140;255m";
const DIM: &str = "\x1b[38;2;138;148;163m";
const LIVE: &str = "\x1b[38;2;62;213;152m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn kind_color(k: ObjectionKind) -> &'static str {
    match k {
        ObjectionKind::Rebut => REBUT,
        ObjectionKind::Comply => COMPLY,
        ObjectionKind::Disqualify => DISQUALIFY,
    }
}

#[derive(Parser)]
#[command(
    name = "riposte",
    about = "Real-time objection coaching for outbound sales agents",
    version
)]
struct Cli {
    /// Database file. Created if absent.
    #[arg(long, global = true, default_value = "riposte.db")]
    db: String,

    /// Use the BGE-small ONNX embedder instead of the built-in lexical one.
    /// Requires a build with `--features onnx`.
    #[arg(long, global = true)]
    onnx: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import, list, and inspect playbooks
    #[command(subcommand)]
    Playbook(PlaybookCmd),

    /// Print the Quick Response Rail — Layer 0, no models involved
    Rail,

    /// Score a line of prospect speech against the active playbook
    Detect(DetectArgs),

    /// Simulate pressing a rail hotkey, recording it as manual use
    Hotkey { slot: u8 },

    /// Rail use vs. detected use — the PRD §7 headline metric
    Stats,
}

#[derive(Subcommand)]
enum PlaybookCmd {
    /// Import a playbook YAML and make it active
    Import { path: String },
    /// List imported playbooks
    List,
}

#[derive(Args)]
struct DetectArgs {
    /// The prospect's words
    #[arg(long)]
    text: String,
    /// Show every objection's score, not just the hit
    #[arg(long)]
    all: bool,
}

fn embedder(use_onnx: bool) -> Result<Box<dyn Embedder>> {
    if !use_onnx {
        return Ok(Box::new(HashEmbedder::new()));
    }
    #[cfg(feature = "onnx")]
    {
        return riposte_detect::OnnxEmbedder::new()
            .map(|e| Box::new(e) as Box<dyn Embedder>)
            .map_err(|e| anyhow::anyhow!("loading BGE-small: {e}"));
    }
    #[cfg(not(feature = "onnx"))]
    anyhow::bail!("this build has no ONNX support — rebuild with `--features onnx`")
}

fn active_playbook(store: &Store) -> Result<(String, Vec<Objection>)> {
    let id = store
        .active_playbook_id()?
        .context("no active playbook — run `riposte playbook import playbooks/insurance-cold-call.yaml`")?;
    let objections = store.load_objections(&id)?;
    Ok((id, objections))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open(&cli.db).with_context(|| format!("opening {}", cli.db))?;

    match cli.command {
        Command::Playbook(PlaybookCmd::Import { path }) => {
            let pb = PlaybookFile::load(&path)?;
            let report = riposte_playbook::import(&store, &pb)?;
            println!(
                "{LIVE}✓{RESET} imported {BOLD}{}{RESET}",
                report.name
            );
            println!(
                "  {} objections · {} on the rail · {} compliance {DIM}(never refined){RESET}",
                report.objections_imported, report.rail_slots_filled, report.comply_entries
            );
            println!("  {DIM}active playbook: {}{RESET}", report.playbook_id);
        }

        Command::Playbook(PlaybookCmd::List) => {
            let rows = store.list_playbooks()?;
            if rows.is_empty() {
                println!("{DIM}no playbooks imported{RESET}");
                return Ok(());
            }
            for p in rows {
                let marker = if p.is_active { format!("{LIVE}●{RESET}") } else { " ".into() };
                println!(
                    "{marker} {BOLD}{}{RESET}  {DIM}v{} · {} objections · {}{RESET}",
                    p.name, p.schema_version, p.objection_count, p.id
                );
            }
        }

        Command::Rail => {
            let (_, objections) = active_playbook(&store)?;
            println!();
            println!("  {DIM}QUICK RESPONSE RAIL{RESET}  {DIM}· no model · no network · 0 ms{RESET}");
            println!();

            let mut rail: Vec<&Objection> =
                objections.iter().filter(|o| o.rail_slot.is_some() && o.enabled).collect();
            rail.sort_by_key(|o| o.rail_slot);

            for o in &rail {
                let c = kind_color(o.kind);
                let slot = o.rail_slot.unwrap_or(0);
                let tag = match o.kind {
                    ObjectionKind::Comply => format!("  {c}COMPLY{RESET}"),
                    ObjectionKind::Disqualify => format!("  {c}DISQUALIFY{RESET}"),
                    ObjectionKind::Rebut => String::new(),
                };
                println!("   {c}▌{slot}▐{RESET}  {}{}", o.label, tag);
            }

            let below = objections.iter().filter(|o| o.rail_slot.is_none() && o.enabled).count();
            println!();
            println!("  {DIM}⌄ more ({below}){RESET}");
            println!();
        }

        Command::Detect(args) => {
            let (_, objections) = active_playbook(&store)?;
            let emb = embedder(cli.onnx)?;
            let started = std::time::Instant::now();
            let gate = DetectionGate::build(&objections, emb.as_ref());
            let build_ms = started.elapsed().as_millis();

            let scored = std::time::Instant::now();
            let scores = gate.score(&args.text, emb.as_ref());
            let gate_us = scored.elapsed().as_micros();

            println!();
            println!("  {DIM}prospect:{RESET} \"{}\"", args.text);
            println!(
                "  {DIM}{} objections · {} · embed+score {}µs · index build {}ms{RESET}",
                gate.objection_count(),
                gate.model_id(),
                gate_us,
                build_ms
            );
            println!();

            let hit = scores.iter().find(|s| s.is_hit());
            match hit {
                Some(s) => {
                    let c = kind_color(s.kind);
                    let objection = objections.iter().find(|o| o.id == s.objection_id).unwrap();
                    println!("  {c}▌●▐{RESET}  {BOLD}{}{RESET}   {c}{}{RESET}", s.label, s.kind.as_str());
                    println!(
                        "        {DIM}score {:.3} ≥ threshold {:.2}  ·  matched \"{}\"{RESET}",
                        s.score, s.threshold, s.best_trigger
                    );
                    println!();
                    for line in objection.response.lines().filter(|l| !l.trim().is_empty()) {
                        println!("        {}", line.trim());
                    }
                    if !objection.talk_track.is_empty() {
                        println!();
                        for t in &objection.talk_track {
                            println!("        {DIM}\"{}\"{RESET}", t);
                        }
                    }
                    if s.kind == ObjectionKind::Comply {
                        println!();
                        println!("        {COMPLY}▲ compliance response — never sent to a model{RESET}");
                    }
                }
                None => {
                    let best = &scores[0];
                    println!("  {DIM}— no hit —{RESET}");
                    println!(
                        "  {DIM}closest: {} at {:.3}, needs {:.2}{RESET}",
                        best.label, best.score, best.threshold
                    );
                }
            }

            if args.all {
                println!();
                println!("  {DIM}{:<28} {:>7} {:>7} {:>7} {:>7}{RESET}", "objection", "pos", "neg", "score", "thresh");
                for s in &scores {
                    let mark = if s.is_hit() { format!("{LIVE}✓{RESET}") } else { " ".into() };
                    println!(
                        "{mark} {:<28} {:>7.3} {:>7.3} {:>7.3} {:>7.2}",
                        s.label, s.positive, s.negative, s.score, s.threshold
                    );
                }
            }
            println!();
        }

        Command::Hotkey { slot } => {
            let (_, objections) = active_playbook(&store)?;
            let o = objections
                .iter()
                .find(|o| o.rail_slot == Some(slot))
                .with_context(|| format!("no objection on rail slot {slot}"))?;

            // Layer 0, origin manual_hotkey, no call and no session. This is
            // the PRD's primary success metric and it must be recordable with
            // nothing else running. See docs/DECISIONS.md D1.
            let card_id = format!("card_{}", riposte_store::now_ms());
            store.record_card(
                &card_id, None, None, &o.id,
                CardOrigin::ManualHotkey, o.kind,
                0, &o.response, None, None, None,
            )?;

            let c = kind_color(o.kind);
            println!();
            println!("  {c}▌{slot}▐{RESET}  {BOLD}{}{RESET}", o.label);
            println!();
            for line in o.response.lines().filter(|l| !l.trim().is_empty()) {
                println!("        {}", line.trim());
            }
            for t in &o.talk_track {
                println!("        {DIM}\"{}\"{RESET}", t);
            }
            println!();
            println!("  {DIM}recorded as manual_hotkey · layer 0{RESET}");
            println!();
        }

        Command::Stats => {
            let split = store.usage_split()?;
            let total = split.manual + split.detected;
            println!();
            println!("  {BOLD}Rail usage{RESET}  {DIM}(docs/01-PRD.md §7){RESET}");
            println!("    manual   {LIVE}{}{RESET}", split.manual);
            println!("    detected {}", split.detected);
            if total > 0 {
                let pct = 100.0 * split.manual as f64 / total as f64;
                println!("    {DIM}{:.0}% of card use was the rail alone{RESET}", pct);
            }
            println!();
        }
    }

    Ok(())
}
