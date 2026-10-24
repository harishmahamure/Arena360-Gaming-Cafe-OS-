//! `arena-e2e`: the test-only end-to-end driver for the Epic 1 flows (Story 1.22). It talks to
//! a running Hub over its real loopback HTTP API and to a console-mode kiosk service over its
//! real app channel (with the service's `dev.*` hooks), so the WAN-down job can run every flow
//! inside a network namespace with no route out.
//!
//! ```text
//! arena-e2e run --hub-data <dir> --kiosk-data <dir> [--health-port <port>]
//!               [--iterations <n> | --hours <h>] --out <result.json>
//! arena-e2e check --hub-data <dir> --result <result.json> [--table <table.jsonl>]
//! arena-e2e compare <table-a.jsonl> <table-b.jsonl>
//! ```
//!
//! - `run` loops the flows (once by default; `--hours` repeats them until the time is up),
//!   then checks the change log (gap-free `seq`, the ledger) and writes the result: the
//!   intents applied per kind before the run and the ledger of those the driver caused.
//! - `check` re-reads the change log later (after the soak restores the network): gap-free
//!   `seq`, ledger counts, and optionally writes the normalised table.
//! - `compare` exits 1 unless the two normalised tables are equal.
//!
//! Exit codes: 0 pass, 1 a flow or a check failed, 64 bad usage.

mod flows;
mod http;
mod ipc;
mod table;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::json;

const USAGE: &str = "usage:
  arena-e2e run --hub-data <dir> --kiosk-data <dir> [--health-port <port>] [--iterations <n> | --hours <h>] --out <result.json>
  arena-e2e check --hub-data <dir> --result <result.json> [--table <table.jsonl>]
  arena-e2e compare <table-a.jsonl> <table-b.jsonl>";

#[derive(Debug, Default, PartialEq)]
struct Args {
    cmd: String,
    hub_data: Option<PathBuf>,
    kiosk_data: Option<PathBuf>,
    health_port: u16,
    iterations: Option<u64>,
    hours: Option<f64>,
    out: Option<PathBuf>,
    result: Option<PathBuf>,
    table: Option<PathBuf>,
    positional: Vec<PathBuf>,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut it = args.into_iter();
    let mut a = Args {
        cmd: it.next().ok_or("no command")?,
        health_port: 7409,
        ..Args::default()
    };
    if !matches!(a.cmd.as_str(), "run" | "check" | "compare") {
        return Err(format!("unknown command {}", a.cmd));
    }
    while let Some(flag) = it.next() {
        if !flag.starts_with("--") {
            a.positional.push(flag.into());
            continue;
        }
        let v = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let num = |v: &str| {
            v.parse::<f64>()
                .map_err(|_| format!("invalid {flag} {v:?}"))
        };
        match flag.as_str() {
            "--hub-data" => a.hub_data = Some(v.into()),
            "--kiosk-data" => a.kiosk_data = Some(v.into()),
            "--health-port" => {
                a.health_port = v.parse().map_err(|_| format!("invalid {flag} {v:?}"))?;
            }
            "--iterations" => {
                a.iterations = Some(
                    v.parse()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("invalid {flag} {v:?}"))?,
                );
            }
            "--hours" => {
                let h = num(&v)?;
                if !(h >= 0.0 && h.is_finite()) {
                    return Err(format!("invalid {flag} {v:?}"));
                }
                a.hours = Some(h);
            }
            "--out" => a.out = Some(v.into()),
            "--result" => a.result = Some(v.into()),
            "--table" => a.table = Some(v.into()),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let need = |o: &Option<PathBuf>, f: &str| {
        if o.is_none() {
            Err(format!("{f} is required"))
        } else {
            Ok(())
        }
    };
    match a.cmd.as_str() {
        "run" => {
            need(&a.hub_data, "--hub-data")?;
            need(&a.kiosk_data, "--kiosk-data")?;
            need(&a.out, "--out")?;
            if a.iterations.is_some() && a.hours.is_some() {
                return Err("--iterations and --hours are exclusive".into());
            }
        }
        "check" => {
            need(&a.hub_data, "--hub-data")?;
            need(&a.result, "--result")?;
        }
        _ => {}
    }
    match a.cmd.as_str() {
        "run" | "check" if !a.positional.is_empty() => {
            return Err(format!(
                "{} takes no positional arguments ({})",
                a.cmd,
                a.positional[0].display()
            ));
        }
        "run" | "check" => {}
        _ => {
            if a.positional.len() != 2 {
                return Err("compare takes two tables".into());
            }
        }
    }
    Ok(a)
}

fn read_json(path: &PathBuf) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn counts(v: &serde_json::Value) -> BTreeMap<String, u64> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, n)| Some((k.clone(), n.as_u64()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Gap-free `seq` and the ledger, then (optionally) the normalised table.
fn check(
    hub_data: &std::path::Path,
    result: &serde_json::Value,
    table_out: Option<&PathBuf>,
) -> Result<(), String> {
    let changes = table::read(&hub_data.join("branch.db"))?;
    table::check_gap_free(&changes)?;
    table::check_no_duplicates(&changes)?;
    table::check_ledger(
        &counts(&result["before"]),
        &table::intents_by_kind(&changes),
        &counts(&result["ledger"]),
    )?;
    if let Some(out) = table_out {
        let lines = table::normalised_table(&changes)?;
        std::fs::write(out, lines.join("\n") + "\n")
            .map_err(|e| format!("{}: {e}", out.display()))?;
    }
    println!(
        "arena-e2e: {} change rows, seq gap-free, ledger matches",
        changes.len()
    );
    Ok(())
}

async fn run(a: &Args) -> Result<(), String> {
    let hub_data = a.hub_data.clone().unwrap_or_default();
    let mut ctx = flows::Ctx::connect(
        a.health_port,
        &hub_data,
        a.kiosk_data.as_deref().unwrap_or(std::path::Path::new(".")),
    )
    .await?;
    let before = table::intents_by_kind(&table::read(&hub_data.join("branch.db"))?);
    let started = Instant::now();
    let deadline = a
        .hours
        .map(|h| started + Duration::from_secs_f64(h * 3600.0));
    let mut n = 0;
    loop {
        n += 1;
        ctx.iteration(n)
            .await
            .map_err(|e| format!("iteration {n}: {e}"))?;
        let more = match (a.iterations, deadline) {
            (Some(max), _) => n < max,
            (None, Some(d)) => Instant::now() < d,
            (None, None) => false,
        };
        if !more {
            break;
        }
    }
    let result = json!({
        "iterations": n,
        "seconds": started.elapsed().as_secs(),
        "before": before,
        "ledger": ctx.ledger,
    });
    let out = a.out.clone().unwrap_or_default();
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&result).unwrap_or_default(),
    )
    .map_err(|e| format!("{}: {e}", out.display()))?;
    println!("arena-e2e: {n} iteration(s) passed");
    check(&hub_data, &result, None)
}

fn main() -> ExitCode {
    let a = match parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("arena-e2e: {e}\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    let outcome = match a.cmd.as_str() {
        "run" => match tokio::runtime::Runtime::new() {
            Ok(rt) => rt.block_on(run(&a)),
            Err(e) => Err(e.to_string()),
        },
        "check" => read_json(a.result.as_ref().unwrap_or(&PathBuf::new())).and_then(|r| {
            check(
                a.hub_data.as_deref().unwrap_or(std::path::Path::new(".")),
                &r,
                a.table.as_ref(),
            )
        }),
        _ => {
            let read = |p: &PathBuf| {
                std::fs::read_to_string(p)
                    .map(|t| t.lines().map(str::to_owned).collect::<Vec<_>>())
                    .map_err(|e| format!("{}: {e}", p.display()))
            };
            read(&a.positional[0]).and_then(|x| {
                let y = read(&a.positional[1])?;
                table::compare(&x, &y)?;
                println!(
                    "arena-e2e: the normalised tables are equal ({} rows)",
                    x.len()
                );
                Ok(())
            })
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arena-e2e: FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Args, String> {
        parse(a.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn commands_parse() {
        let a = p(&[
            "run",
            "--hub-data",
            "/h",
            "--kiosk-data",
            "/k",
            "--hours",
            "0.05",
            "--out",
            "/r",
        ])
        .unwrap();
        assert_eq!(a.hours, Some(0.05));
        assert_eq!(a.health_port, 7409);
        let a = p(&[
            "check",
            "--hub-data",
            "/h",
            "--result",
            "/r",
            "--table",
            "/t",
        ])
        .unwrap();
        assert_eq!(a.table, Some("/t".into()));
        assert_eq!(p(&["compare", "a", "b"]).unwrap().positional.len(), 2);
        for bad in [
            &[][..],
            &["nope"],
            &["run", "--hub-data", "/h"],
            &[
                "run",
                "--hub-data",
                "/h",
                "--kiosk-data",
                "/k",
                "--out",
                "/r",
                "--iterations",
                "0",
            ],
            &[
                "run",
                "--hub-data",
                "/h",
                "--kiosk-data",
                "/k",
                "--out",
                "/r",
                "--iterations",
                "1",
                "--hours",
                "1",
            ],
            &[
                "run",
                "--hub-data",
                "/h",
                "--kiosk-data",
                "/k",
                "--out",
                "/r",
                "--hours",
                "-1",
            ],
            &["check", "--hub-data", "/h"],
            &["compare", "a"],
            &["run", "--bogus", "x"],
            &["check", "--hub-data", "/h", "--result", "/r", "stray"],
            &[
                "run",
                "stray",
                "--hub-data",
                "/h",
                "--kiosk-data",
                "/k",
                "--out",
                "/r",
            ],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }
}
