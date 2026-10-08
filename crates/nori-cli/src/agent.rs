//! `nori-cli agent "<request>"`: nori's built-in agent on a file or in memory, without the app.
//!
//! The same agent as the Agent panel (the harness's brief, skills, live context, checks, one
//! checkpoint per turn), with any of its providers. Claude Code and Codex drive this process's
//! session through `nori-mcp --live` on a private bridge (its own control file, so a running nori
//! is never disturbed). The evals (`evals/`) run every job through here.

use std::io::Write;
use std::time::Instant;

use nori_agent::{Agent, AgentConfig, AgentEvent, Conversation, ProviderKind};
use nori_cli::{Backend, Invocation, nori_control};
use nori_control::Source;
use serde_json::json;

use crate::{Failure, Res};

const FLAGS: &[&str] = &["--provider", "--model", "--max-steps", "--transcript"];

pub async fn run(inv: &Invocation) -> Res {
    let mut words = vec![];
    let mut i = 0;
    while i < inv.rest.len() {
        let a = inv.rest[i].as_str();
        if FLAGS.contains(&a) {
            i += 2;
            continue;
        }
        if a != "--json" {
            if a.starts_with("--") {
                return Err(Failure::Usage(format!("Unknown option `{a}` for agent (--provider, --model, --max-steps, --transcript, --json)")));
            }
            words.push(a.to_string());
        }
        i += 1;
    }
    let prompt = words.join(" ");
    if prompt.trim().is_empty() {
        return Err(Failure::Usage("agent needs a request: nori-cli --file poster.nori agent \"Make an A3 poster for …\"".into()));
    }
    let json_out = crate::has(inv, "--json");
    let backend = match (&inv.file, inv.headless) {
        (Some(path), _) => Backend::file(path, Source::Cli).await?,
        (None, true) => Backend::headless(Source::Cli).await?,
        _ => {
            return Err(Failure::Usage(
                "agent runs the agent in this process: give --file <file> (or --headless). To ask the running app's agent, use `nori-cli agent.send prompt=…`.".into(),
            ));
        }
    };
    let Backend::Local { session, .. } = &backend else { unreachable!("a local backend") };

    let mut config = AgentConfig::from_settings(&session.settings().agent);
    if let Some(p) = crate::flag_value(inv, "--provider")? {
        config.provider = ProviderKind::parse(p).ok_or_else(|| Failure::Usage(format!("`{p}` isn't a provider: lsuite, claude-code, codex, anthropic, openai, ollama, openai-compatible")))?;
        config.model = String::new();
        config.base_url = String::new();
    }
    if let Some(m) = crate::flag_value(inv, "--model")? {
        config.model = m.to_string();
    }
    if let Some(n) = crate::flag_value(inv, "--max-steps")? {
        config.max_steps = n.parse().map_err(|_| Failure::Usage("--max-steps is a number".into()))?;
    }
    // Claude Code and Codex reach this session through nori-mcp --live: a bridge of its own.
    let _bridge = if config.provider.is_cli() {
        let path = session.data_dir.join("agent").join(format!("control-cli-{}.json", std::process::id()));
        Some(nori_control::bridge::Server::start_at(session.clone(), path).await.map_err(|e| Failure::Command(format!("Couldn't start the bridge for {}: {e}", config.provider.label())))?)
    } else {
        None
    };

    let started = Instant::now();
    let provider = config.provider;
    let model = config.model();
    let mut run = Agent::start(session, config, prompt.clone(), Conversation::new());
    let (mut reply, mut commands, mut usage) = (String::new(), vec![], (0u64, 0u64));
    let mut outcome: Result<(Option<u64>, usize), String> = Err("The agent stopped without an answer.".into());
    let mut conversation = None;
    while let Some(event) = run.next_event().await {
        match event {
            AgentEvent::Status { message } if !json_out => eprintln!("· {message}"),
            AgentEvent::Status { .. } => {}
            AgentEvent::Text { delta } => {
                reply.push_str(&delta);
                if !json_out {
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(delta.as_bytes());
                    let _ = out.flush();
                }
            }
            AgentEvent::Command { record, .. } => {
                if !json_out {
                    eprintln!("▸ {} {}", record.command, if record.ok { "ok".to_string() } else { format!("failed: {}", record.error.clone().unwrap_or_default()) });
                }
                let mut c = json!({ "command": record.command, "ok": record.ok, "error": record.error, "mutates": record.mutates, "source": record.source });
                if record.command == "harness.skill" {
                    c["skill"] = record.params.get("name").cloned().unwrap_or_default();
                }
                commands.push(c);
            }
            AgentEvent::Usage { input_tokens, output_tokens } => {
                usage.0 += input_tokens;
                usage.1 += output_tokens;
            }
            AgentEvent::Done { summary, checkpoint, changes, conversation: c } => {
                if reply.trim().is_empty() {
                    reply = summary;
                }
                conversation = Some(c);
                outcome = Ok((checkpoint, changes));
            }
            AgentEvent::Error { message, .. } => outcome = Err(message),
            AgentEvent::Cancelled { .. } => outcome = Err("Cancelled.".into()),
        }
    }
    if !json_out && !reply.ends_with('\n') {
        println!();
    }
    // A --file session saves after every change; one the agent made with doc.new saves here.
    let mut saved = None;
    if let Some(path) = backend.path()
        && session.is_open()
    {
        let target = nori_cli::save_target(path);
        match session.save(Some(target.clone())) {
            Ok(p) => saved = Some(p),
            Err(e) => eprintln!("warning: couldn't save {}: {e}", target.display()),
        }
    }
    if let (Some(path), Some(c)) = (crate::flag_value(inv, "--transcript")?, &conversation) {
        let _ = std::fs::write(path, serde_json::to_vec_pretty(c).unwrap_or_default());
    }
    let seconds = (started.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    let (checkpoint, changes) = outcome.as_ref().map(|(c, n)| (*c, *n)).unwrap_or((None, commands.iter().filter(|c| c["ok"] == true && c["mutates"] == true).count()));
    if json_out {
        let report = json!({
            "ok": outcome.is_ok(),
            "reply": reply.trim(),
            "error": outcome.as_ref().err(),
            "provider": provider.id(),
            "model": model,
            "changes": changes,
            "checkpoint": checkpoint,
            "commands": commands,
            "usage": { "inputTokens": usage.0, "outputTokens": usage.1 },
            "seconds": seconds,
            "file": saved.map(|p| p.display().to_string()),
        });
        crate::print_json(&report, inv.compact);
    }
    match outcome {
        Ok(_) => Ok(()),
        Err(e) => Err(Failure::Command(e)),
    }
}
