//! nori-evals: the agent harness's evals (lsuite's HARNESS.md part 7).
//!
//! Each job in `evals/jobs/*.json` is a design job of the trade: a fixture (a blank start, or
//! files and commands that make a starting document), a request in a designer's words, and
//! automatic checks on the resulting document (structure, text, sizes, contrast, bounds, files
//! written, pixels). The runner makes the fixture, runs the request headless through
//! `nori-cli --file job.nori agent … --json` with a real model (the Claude Code provider by
//! default: no key needed on a developer's machine; `--provider anthropic` reads
//! `ANTHROPIC_API_KEY`), then scores the saved document and the agent's report.
//!
//! ```text
//! cargo build -p nori-cli -p nori-mcp -p nori-evals
//! target/debug/nori-evals [--provider claude-code] [--model sonnet] [--only poster,logo]
//!                         [--parallel 2] [--max-steps 40] [--timeout 900] [--record] [--list]
//!                         [--score-only evals/runs/<run>]
//! ```
//!
//! Every job gets its own folders (nori's data, config and `LSUITE_HOME`) under
//! `evals/runs/<date>-<model>/<job>/`: the fixture, `job.nori`, `out/` (exports), `agent.json`
//! (the agent's report), `agent.log`, `transcript.json` and `score.json`. `--record` adds the run
//! to `evals/RESULTS.md`.

mod fixtures;
mod score;

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One eval job (`jobs/<name>.json`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub name: String,
    pub title: String,
    /// The skill a good agent loads for it (reported, not enforced).
    #[serde(default)]
    pub skill: Option<String>,
    /// Pictures the fixture makes before the setup runs: `{path, make, width?, height?}`.
    #[serde(default)]
    pub files: Vec<fixtures::FileSpec>,
    /// Commands that make the starting document (none: the agent starts from nothing).
    #[serde(default)]
    pub setup: Vec<Value>,
    /// The request; `{out}` is the folder for exports, `{dir}` the job's folder.
    pub prompt: String,
    pub checks: Vec<Value>,
    #[serde(default)]
    pub max_steps: Option<usize>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Clone, Debug)]
struct Config {
    provider: String,
    model: String,
    parallel: usize,
    max_steps: usize,
    timeout: u64,
    only: Vec<String>,
    record: bool,
    jobs_dir: PathBuf,
}

/// What a job scored.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub name: String,
    pub title: String,
    pub passed: bool,
    pub checks_passed: usize,
    pub checks_total: usize,
    pub checks: Vec<score::CheckResult>,
    pub agent_ok: bool,
    pub agent_error: Option<String>,
    pub reply: String,
    pub commands: usize,
    pub changes: usize,
    pub skills_loaded: Vec<String>,
    pub looked: bool,
    pub seconds: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn usage() -> ! {
    eprintln!("{}", USAGE);
    std::process::exit(2)
}

const USAGE: &str = "nori-evals — run nori's agent evals (see evals/src/main.rs)

  nori-evals [--provider claude-code] [--model M] [--only a,b] [--parallel N] [--max-steps N]
             [--timeout SECS] [--record] [--jobs DIR]
  nori-evals --list
  nori-evals --score-only <run folder> [--record]

Build nori-cli and nori-mcp first: cargo build -p nori-cli -p nori-mcp -p nori-evals";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_jobs(dir: &Path) -> Result<Vec<Job>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    paths.sort();
    paths.iter().map(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Job>(&t).map_err(|e| format!("{}: {e}", p.display())))).collect()
}

/// `nori-cli` (and `nori-mcp`) next to this program.
fn sibling(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().unwrap_or(Path::new(".")).join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config { provider: "claude-code".into(), model: String::new(), parallel: 1, max_steps: 40, timeout: 900, only: vec![], record: false, jobs_dir: manifest_dir().join("jobs") };
    let (mut list, mut score_only) = (false, None::<PathBuf>);
    let mut i = 0;
    let value = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().unwrap_or_else(|| usage())
    };
    while i < args.len() {
        match args[i].as_str() {
            "--provider" => cfg.provider = value(&mut i),
            "--model" => cfg.model = value(&mut i),
            "--only" => cfg.only = value(&mut i).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            "--parallel" => cfg.parallel = value(&mut i).parse().unwrap_or_else(|_| usage()),
            "--max-steps" => cfg.max_steps = value(&mut i).parse().unwrap_or_else(|_| usage()),
            "--timeout" => cfg.timeout = value(&mut i).parse().unwrap_or_else(|_| usage()),
            "--jobs" => cfg.jobs_dir = PathBuf::from(value(&mut i)),
            "--record" => cfg.record = true,
            "--list" => list = true,
            "--score-only" => score_only = Some(PathBuf::from(value(&mut i))),
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("unknown option {other}");
                usage()
            }
        }
        i += 1;
    }
    let jobs = match load_jobs(&cfg.jobs_dir) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("nori-evals: {e}");
            std::process::exit(1)
        }
    };
    if list {
        for j in &jobs {
            println!("{:<18} {} ({} checks)", j.name, j.title, j.checks.len());
        }
        return;
    }
    let jobs: Vec<Job> = jobs.into_iter().filter(|j| cfg.only.is_empty() || cfg.only.contains(&j.name)).collect();
    if jobs.is_empty() {
        eprintln!("nori-evals: no job matches --only {}", cfg.only.join(","));
        std::process::exit(2);
    }

    let (run_dir, results) = match score_only {
        Some(dir) => {
            let meta: Value = std::fs::read(dir.join("run.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            cfg.provider = meta["provider"].as_str().unwrap_or(&cfg.provider).to_string();
            cfg.model = meta["model"].as_str().unwrap_or(&cfg.model).to_string();
            set_home(&dir);
            let mut results = vec![];
            for job in &jobs {
                let jd = dir.join(&job.name);
                if !jd.is_dir() {
                    continue;
                }
                let baseline = fixtures::load_baseline(&jd);
                let report: Value = std::fs::read(jd.join("agent.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                let r = score::score(job, &jd, &baseline, &report).await;
                let _ = std::fs::write(jd.join("score.json"), serde_json::to_vec_pretty(&r).unwrap_or_default());
                print_result(&r);
                results.push(r);
            }
            (dir, results)
        }
        None => {
            let (cli, mcp) = (sibling("nori-cli"), sibling("nori-mcp"));
            for p in [&cli, &mcp] {
                if !p.is_file() {
                    eprintln!("nori-evals: {} is missing: cargo build -p nori-cli -p nori-mcp -p nori-evals", p.display());
                    std::process::exit(1);
                }
            }
            let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M").to_string();
            let label = if cfg.model.is_empty() { cfg.provider.clone() } else { format!("{}-{}", cfg.provider, cfg.model) };
            let run_dir = manifest_dir().join("runs").join(format!("{stamp}-{label}"));
            std::fs::create_dir_all(&run_dir).expect("the run folder");
            set_home(&run_dir);
            let _ = std::fs::write(run_dir.join("run.json"), serde_json::to_vec_pretty(&json!({ "provider": cfg.provider, "model": cfg.model, "started": chrono::Local::now().to_rfc3339(), "jobs": jobs.iter().map(|j| &j.name).collect::<Vec<_>>() })).unwrap_or_default());
            eprintln!("nori-evals: {} job{} with {} in {}", jobs.len(), if jobs.len() == 1 { "" } else { "s" }, label, run_dir.display());
            let results: Vec<JobResult> = futures::stream::iter(jobs.iter().cloned())
                .map(|job| {
                    let (cfg, run_dir, cli, mcp) = (cfg.clone(), run_dir.clone(), cli.clone(), mcp.clone());
                    async move {
                        let r = run_job(&job, &cfg, &run_dir, &cli, &mcp).await;
                        print_result(&r);
                        r
                    }
                })
                .buffered(cfg.parallel.max(1))
                .collect()
                .await;
            (run_dir, results)
        }
    };
    let passed = results.iter().filter(|r| r.passed).count();
    let checks: (usize, usize) = results.iter().fold((0, 0), |a, r| (a.0 + r.checks_passed, a.1 + r.checks_total));
    println!("\n{passed}/{} jobs passed ({:.0} %), {}/{} checks ({:.0} %). Run folder: {}", results.len(), pct(passed, results.len()), checks.0, checks.1, pct(checks.0, checks.1), run_dir.display());
    let _ = std::fs::write(run_dir.join("results.json"), serde_json::to_vec_pretty(&results).unwrap_or_default());
    if cfg.record {
        match record(&cfg, &run_dir, &results) {
            Ok(p) => println!("Recorded in {}", p.display()),
            Err(e) => eprintln!("nori-evals: couldn't record the results: {e}"),
        }
    }
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 }
}

/// The runner's own `LSUITE_HOME` (fixtures and scoring run in this process).
fn set_home(run_dir: &Path) {
    let home = run_dir.join(".lsuite");
    let _ = std::fs::create_dir_all(&home);
    // SAFETY: set at start, before any session reads it; never changed afterwards.
    unsafe {
        std::env::set_var("LSUITE_HOME", &home);
        std::env::set_var("NORI_NO_SYSTEM_FONTS", "1");
        std::env::set_var("NORI_NO_UPDATE", "1");
    }
}

fn print_result(r: &JobResult) {
    let failed: Vec<&str> = r.checks.iter().filter(|c| !c.ok).map(|c| c.label.as_str()).collect();
    println!(
        "{} {:<18} {}/{} checks, {} commands, {:.0} s{}{}",
        if r.passed { "PASS" } else { "FAIL" },
        r.name,
        r.checks_passed,
        r.checks_total,
        r.commands,
        r.seconds,
        if failed.is_empty() { String::new() } else { format!(" — failed: {}", failed.join(", ")) },
        r.agent_error.as_ref().map(|e| format!(" — agent: {}", e.chars().take(160).collect::<String>())).unwrap_or_default(),
    );
}

async fn run_job(job: &Job, cfg: &Config, run_dir: &Path, cli: &Path, mcp: &Path) -> JobResult {
    let dir = run_dir.join(&job.name);
    let out = dir.join("out");
    let _ = std::fs::create_dir_all(&out);
    let baseline = match fixtures::prepare(job, &dir).await {
        Ok(b) => b,
        Err(e) => {
            let mut r = score::failed(job, format!("The fixture failed: {e}"));
            r.agent_error = Some(format!("fixture: {e}"));
            return r;
        }
    };
    let prompt = job.prompt.replace("{out}", &out.display().to_string()).replace("{dir}", &dir.display().to_string());
    let mut cmd = tokio::process::Command::new(cli);
    cmd.arg("--file").arg(dir.join("job.nori")).arg("--compact").arg("agent").arg(&prompt).arg("--json").arg("--provider").arg(&cfg.provider);
    if !cfg.model.is_empty() {
        cmd.arg("--model").arg(&cfg.model);
    }
    cmd.arg("--max-steps").arg(job.max_steps.unwrap_or(cfg.max_steps).to_string()).arg("--transcript").arg(dir.join("transcript.json"));
    for (k, v) in [("NORI_DATA_DIR", dir.join("data")), ("NORI_CONFIG_DIR", dir.join("config")), ("LSUITE_HOME", dir.join("home")), ("NORI_MCP", mcp.to_path_buf())] {
        let _ = std::fs::create_dir_all(if k == "NORI_MCP" { &dir } else { &v });
        cmd.env(k, v);
    }
    cmd.env("NORI_NO_UPDATE", "1").env("NORI_NO_SYSTEM_FONTS", "1").env_remove("NORI_CONTROL").current_dir(&dir).kill_on_drop(true);
    let log = std::fs::File::create(dir.join("agent.log")).ok();
    cmd.stdout(std::process::Stdio::piped()).stderr(log.map(std::process::Stdio::from).unwrap_or_else(std::process::Stdio::null));
    eprintln!("· {}: running the agent…", job.name);
    let started = std::time::Instant::now();
    let timeout = Duration::from_secs(job.timeout.unwrap_or(cfg.timeout));
    let report: Value = match cmd.spawn() {
        Err(e) => json!({ "ok": false, "error": format!("couldn't start nori-cli: {e}") }),
        Ok(child) => match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Err(_) => json!({ "ok": false, "error": format!("timed out after {} s", timeout.as_secs()) }),
            Ok(Err(e)) => json!({ "ok": false, "error": e.to_string() }),
            Ok(Ok(o)) => {
                let text = String::from_utf8_lossy(&o.stdout);
                text.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok()).unwrap_or_else(|| json!({ "ok": false, "error": format!("no report from nori-cli (exit {:?})", o.status.code()) }))
            }
        },
    };
    let mut report = report;
    if report.get("seconds").is_none() {
        report["seconds"] = json!(started.elapsed().as_secs_f64());
    }
    let _ = std::fs::write(dir.join("agent.json"), serde_json::to_vec_pretty(&report).unwrap_or_default());
    let r = score::score(job, &dir, &baseline, &report).await;
    let _ = std::fs::write(dir.join("score.json"), serde_json::to_vec_pretty(&r).unwrap_or_default());
    r
}

/// Adds the run to `evals/RESULTS.md`, newest first under the heading.
fn record(cfg: &Config, run_dir: &Path, results: &[JobResult]) -> Result<PathBuf, String> {
    let path = manifest_dir().join("RESULTS.md");
    let old = std::fs::read_to_string(&path).unwrap_or_else(|_| "# nori agent evals\n\n".into());
    let passed = results.iter().filter(|r| r.passed).count();
    let checks: (usize, usize) = results.iter().fold((0, 0), |a, r| (a.0 + r.checks_passed, a.1 + r.checks_total));
    let model = if cfg.model.is_empty() { "its default model".to_string() } else { format!("`{}`", cfg.model) };
    let mut s = format!(
        "## {} · nori {} · {} with {}\n\n{passed}/{} jobs passed ({:.0} %), {}/{} checks ({:.0} %). Run: `{}`.\n\n| Job | Result | Checks | Commands | Skill loaded | Looked | Time | Failed checks |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        env!("CARGO_PKG_VERSION"),
        cfg.provider,
        model,
        results.len(),
        pct(passed, results.len()),
        checks.0,
        checks.1,
        pct(checks.0, checks.1),
        run_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    );
    for r in results {
        let failed: Vec<String> = r.checks.iter().filter(|c| !c.ok).map(|c| c.label.clone()).collect();
        let mut failed = failed.join(", ");
        if let Some(e) = &r.agent_error {
            failed = format!("{failed}{}agent: {}", if failed.is_empty() { "" } else { "; " }, e.chars().take(100).collect::<String>().replace('|', "/"));
        }
        s.push_str(&format!(
            "| {} | {} | {}/{} | {} | {} | {} | {:.0} s | {} |\n",
            r.name,
            if r.passed { "pass" } else { "fail" },
            r.checks_passed,
            r.checks_total,
            r.commands,
            if r.skills_loaded.is_empty() { "—".to_string() } else { r.skills_loaded.join(", ") },
            if r.looked { "yes" } else { "no" },
            r.seconds,
            if failed.is_empty() { "—".into() } else { failed },
        ));
    }
    s.push('\n');
    // After the file's introduction (everything before the first run).
    let at = old.find("\n## ").map(|i| i + 1).unwrap_or(old.len());
    let text = format!("{}{s}{}", &old[..at], &old[at..]);
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}
