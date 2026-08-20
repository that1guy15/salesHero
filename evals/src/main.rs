//! Gate calibration — M1.5.
//!
//! `docs/PROJECT-STATUS.md` §3.1 records a single observation that the shipped
//! thresholds may be badly wrong, and §5 makes measuring that the first step
//! before any more pipeline is built. Until this report exists, `01-PRD.md` §7's
//! precision and recall targets are unevidenced.
//!
//!   riposte-evals calibrate --playbook playbooks/insurance-cold-call.yaml
//!   riposte-evals check     --playbook playbooks/insurance-cold-call.yaml

mod probes;
mod report;
mod scoring;
mod stats;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use riposte_core::Objection;
use riposte_playbook::PlaybookFile;

#[derive(Parser)]
#[command(name = "riposte-evals", about = "Measure the detection gate", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Score matrix, distribution separation, and thresholds proposed from data
    Calibrate {
        #[arg(long, default_value = "playbooks/insurance-cold-call.yaml")]
        playbook: String,
        #[arg(long, default_value = "evals/fixtures/probes.yaml")]
        probes: String,
        #[arg(long, value_enum, default_value_t = Which::Lexical)]
        embedder: Which,
    },
    /// Assert M1's acceptance criteria. Exits non-zero on failure — this is the
    /// CI regression gate the criteria were always meant to be.
    Check {
        #[arg(long, default_value = "playbooks/insurance-cold-call.yaml")]
        playbook: String,
        #[arg(long, default_value = "evals/fixtures/probes.yaml")]
        probes: String,
        #[arg(long, value_enum, default_value_t = Which::Lexical)]
        embedder: Which,
        /// Minimum recall on held-out paraphrases. Not enforced by default:
        /// `docs/DECISIONS.md` D6 states plainly that the lexical embedder is a
        /// CI fixture with near-zero paraphrase recall, so a recall floor here
        /// would fail by design rather than catch a regression.
        #[arg(long)]
        min_recall: Option<f32>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Which {
    /// HashEmbedder — deterministic, no model, no network
    Lexical,
    /// BGE-small-en-v1.5. Requires --features onnx; downloads weights
    Bge,
    /// Both, combined at the score level
    Hybrid,
}

fn load(playbook: &str) -> Result<Vec<Objection>> {
    let pb = PlaybookFile::load(playbook).with_context(|| format!("loading {playbook}"))?;
    Ok(pb
        .objections
        .iter()
        .enumerate()
        .map(|(i, e)| e.to_objection(i as i64))
        .filter(|o| o.enabled)
        .collect())
}

fn arm(which: Which, objections: &[Objection]) -> Result<scoring::Arm> {
    match which {
        Which::Lexical => Ok(scoring::Arm::lexical(objections)),
        #[cfg(feature = "onnx")]
        Which::Bge => scoring::Arm::semantic(objections),
        #[cfg(feature = "onnx")]
        Which::Hybrid => scoring::Arm::hybrid(objections),
        #[cfg(not(feature = "onnx"))]
        _ => anyhow::bail!(
            "this build has no ONNX support — rebuild with `--features onnx`.\n\
             The lexical arm needs no model and runs anywhere; see docs/DECISIONS.md D6."
        ),
    }
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Calibrate {
            playbook,
            probes,
            embedder,
        } => {
            let objections = load(&playbook)?;
            let probe_file = probes::ProbeFile::load(&probes)?;
            let mut a = arm(embedder, &objections)?;
            a.set_floor_from(
                &probe_file
                    .probes
                    .iter()
                    .filter(|p| p.expected().is_none())
                    .map(|p| p.text.clone())
                    .collect::<Vec<_>>(),
            );
            report::calibrate(&a, &objections, &probe_file.probes);
        }
        Command::Check {
            playbook,
            probes,
            embedder,
            min_recall,
        } => {
            let objections = load(&playbook)?;
            let probe_file = probes::ProbeFile::load(&probes)?;
            let mut a = arm(embedder, &objections)?;
            a.set_floor_from(
                &probe_file
                    .probes
                    .iter()
                    .filter(|p| p.expected().is_none())
                    .map(|p| p.text.clone())
                    .collect::<Vec<_>>(),
            );
            let failures = report::check(&a, &objections, &probe_file.probes, min_recall);
            if failures > 0 {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
