//! The registry as model tools, the system prompt, and running one tool call.

use nori_control::session::Event;
use nori_control::vision::Picture;
use nori_control::{CmdResult, CommandRecord, Perm, Source, Spec};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::Run;

/// Largest tool result handed back to the model, in bytes.
pub const TOOL_OUTPUT_LIMIT: usize = 12_000;

/// What the Agent panel adds to the harness's expert brief.
const PANEL: &str = "## In the Agent panel\n\
Each request starts with a <context> block: what the person sees in nori as they ask. Before a later step you get a fresh <context> when the document changed, including edits the person made in the window meanwhile: build on them, never undo them. \"This\", \"here\" and \"the selected layer\" mean what it lists, by id. You can see: harness_look, page_look and layer_look return the picture itself.\n\
Never open, save, close or export files, place images from disk or change settings unless the person asks for exactly that. When the person asks for a plugin (a filter or effect nori doesn't have), follow the write-plugin skill; if the plugins permission is off, say how to turn it on. Answer briefly, in the person's language, without tool names or JSON.";

/// Standing instructions for every provider of the Agent panel (also given to Claude Code and
/// Codex): the harness's expert brief (`harness.brief`, the same source as `nori-mcp`'s
/// instructions), then what is particular to the panel.
pub fn system_prompt() -> &'static str {
    static PROMPT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PROMPT.get_or_init(|| format!("{}\n\n{PANEL}", nori_control::harness::brief()))
}

/// One registry command as a model tool.
#[derive(Clone, Debug)]
pub struct ToolDef {
    /// `family_verb`.
    pub name: String,
    /// `family.verb`.
    pub command: &'static str,
    pub description: &'static str,
    /// JSON Schema of the parameters (`nori_control::input_schema`).
    pub schema: Value,
}

/// Every registry command an agent may ever run, in registry order (stable, so prompt caches
/// hold). Person-only commands are left out: an agent is always refused them. So is the `agent`
/// family: the built-in agent doesn't drive itself.
pub fn tool_defs() -> Vec<ToolDef> {
    nori_control::specs()
        .iter()
        .filter(|s| s.perm != Perm::PersonOnly && s.family() != "agent")
        .map(|s| ToolDef { name: s.tool_name(), command: s.name, description: s.doc, schema: nori_control::input_schema(s) })
        .collect()
}

/// The tool that runs any command by name, for providers that take fewer tools than nori has
/// commands (see [`ToolSet`]).
pub const RUN_TOOL: &str = "nori_run";

/// Commands that stay tools of their own when a provider caps the number of tools: the editing
/// core. Everything else is reached through [`RUN_TOOL`]. The set keeps the registry's order.
pub(crate) const CORE: &[&str] = &[
    "doc.overview", "doc.get", "doc.new", "doc.setInfo", "doc.resize", "doc.resizeCanvas", "doc.crop", "doc.rotate", "doc.batch",
    "page.list", "page.add", "page.remove", "page.move", "page.select", "page.update", "page.addGuide", "page.look",
    "layer.list", "layer.get", "layer.add", "layer.addAdjustment", "layer.place", "layer.update", "layer.select", "layer.move", "layer.align",
    "layer.reorder", "layer.duplicate", "layer.delete", "layer.group", "layer.ungroup", "layer.merge", "layer.rasterize", "layer.setAdjustment",
    "layer.addMask", "layer.mask", "layer.transform", "layer.look",
    "raster.stroke", "raster.fill", "raster.gradient", "raster.clear", "raster.pick",
    "vector.addShape", "vector.addPath", "vector.update", "vector.setGeometry", "vector.toPath", "vector.combine",
    "text.add", "text.update", "text.setRuns", "text.thread", "text.styles", "text.defineStyle", "text.applyStyle", "text.fonts",
    "select.get", "select.all", "select.none", "select.invert", "select.rect", "select.ellipse", "select.polygon", "select.color", "select.layer", "select.modify",
    "filter.list", "filter.apply", "filter.adjust",
    "color.get", "color.set",
    "history.list", "history.undo", "history.redo",
    "export.formats", "export.file",
    "plugin.list", "plugin.guide",
    "app.commands", "ui.state",
    "harness.skill", "harness.skills", "harness.look", "harness.check", "harness.context",
];

/// Commands for small local models (a short tool list keeps their context free for the work).
pub(crate) const COMPACT: &[&str] = &[
    "doc.overview", "doc.batch", "layer.list", "layer.add", "layer.addAdjustment", "layer.update", "layer.move", "layer.delete",
    "raster.stroke", "raster.fill", "vector.addShape", "vector.update", "text.add", "text.update",
    "select.rect", "select.none", "filter.apply", "page.look", "history.undo", "app.commands",
    "harness.skill", "harness.look",
];

/// The tools one provider gets: every command when they fit, else a core set and [`RUN_TOOL`].
#[derive(Clone, Debug)]
pub struct ToolSet {
    pub defs: Vec<ToolDef>,
    /// Some commands are only reachable through [`RUN_TOOL`].
    pub trimmed: bool,
}

impl ToolSet {
    /// At most `limit` tools (`None`: no limit). `compact`: the short list for small models.
    pub fn new(limit: Option<usize>, compact: bool) -> Self {
        let all = tool_defs();
        let limit = limit.unwrap_or(usize::MAX);
        if !compact && all.len() <= limit {
            return Self { defs: all, trimmed: false };
        }
        let keep: &[&str] = if compact { COMPACT } else { CORE };
        let mut defs: Vec<ToolDef> = all.into_iter().filter(|t| keep.contains(&t.command)).take(limit.saturating_sub(1)).collect();
        defs.push(run_tool_def());
        Self { defs, trimmed: true }
    }

    /// What the model is told, with how to reach the other commands when the set is trimmed.
    pub fn system_prompt(&self) -> String {
        if self.trimmed {
            format!("{}\nOnly the most used commands are tools here. Run any other command with {RUN_TOOL} (command: its name, like \"text.thread\"; params: its parameters); app_commands describes every command and its parameters.", system_prompt())
        } else {
            system_prompt().to_string()
        }
    }
}

fn run_tool_def() -> ToolDef {
    ToolDef {
        name: RUN_TOOL.into(),
        command: "",
        description: "Run any nori command by name, for the commands that aren't tools of their own here. app_commands lists them with their parameters.",
        schema: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command's name, family.verb, e.g. text.thread." },
                "params": { "type": "object", "description": "The command's parameters." },
            },
            "required": ["command"],
        }),
    }
}

/// The command behind a tool name: `layer_add`, `layer.add` or `mcp__nori__layer_add`.
pub fn spec_for_tool(name: &str) -> Option<&'static Spec> {
    let name = name.trim().trim_start_matches("mcp__nori__");
    nori_control::specs().iter().find(|s| s.name == name || s.tool_name() == name)
}

/// `value` cut to `limit` bytes on a character boundary, with a note when cut.
pub fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… truncated: the full result is {} bytes. Ask for less (a narrower query, or one item with its *_get command).", &value[..end], value.len())
}

/// The text a model gets back for a command's result, and whether it is an error.
pub fn tool_output(result: &CmdResult) -> (String, bool) {
    match result {
        Ok(v) => (bounded(&serde_json::to_string(v).unwrap_or_default(), TOOL_OUTPUT_LIMIT), false),
        Err(e) => (bounded(e, TOOL_OUTPUT_LIMIT), true),
    }
}

/// What a tool call gives back to the model.
pub(crate) struct Ran {
    pub output: String,
    pub is_error: bool,
    /// Pictures the command pointed at (`page.look`, `layer.look`), for a model that can see.
    pub pictures: Vec<Picture>,
}

impl Ran {
    fn error(output: String) -> Self {
        Self { output, is_error: true, pictures: vec![] }
    }
}

impl Run {
    /// Runs one tool call as `Source::Agent` through the registry (permissions apply there),
    /// shows its card and returns what the model sees. Never fails: errors, refusals included,
    /// go back to the model as tool errors.
    pub async fn run_tool(&mut self, name: &str, input: Result<Value, String>) -> Ran {
        // The catch-all tool names its command in the arguments.
        let (name, input) = match (name, input) {
            (RUN_TOOL, Ok(v)) => {
                let Some(command) = v["command"].as_str().map(str::to_string) else {
                    return Ran::error(format!("{RUN_TOOL} needs \"command\": the command's name, like \"layer.add\"."));
                };
                let params = v.get("params").cloned().unwrap_or(json!({}));
                (command, Ok(params))
            }
            (n, i) => (n.to_string(), i),
        };
        let name = name.as_str();
        let Some(spec) = spec_for_tool(name) else {
            let dotted = name.contains('.');
            let names: Vec<String> = nori_control::specs().iter().map(|s| if dotted { s.name.to_string() } else { s.tool_name() }).collect();
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let hint = nori_control::registry::closest(name, &names).map(|c| format!(" Did you mean {c}?")).unwrap_or_default();
            return Ran::error(format!("There is no {} {name}.{hint}", if dotted { "command" } else { "tool" }));
        };
        if spec.family() == "agent" {
            return Ran::error("The agent can't drive the Agent panel itself.".into());
        }
        let input = match input {
            Ok(Value::Null) => json!({}),
            Ok(v) => v,
            Err(e) => return Ran::error(format!("The arguments for {name} weren't valid JSON ({e}). Send them again as one JSON object.")),
        };
        if spec.mutates {
            self.ensure_checkpoint().await;
        }
        // Drop anything queued so the record found below is this call's.
        while !matches!(self.commands.try_recv(), Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed)) {}
        let result = nori_control::call(&self.session, Source::Agent, spec.name, input.clone()).await;
        let mut record = None;
        loop {
            match self.commands.try_recv() {
                Ok(Event::Command { record: r }) if r.source == Source::Agent && r.command == spec.name => {
                    record = Some(r);
                    break;
                }
                Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
        let record = record.unwrap_or_else(|| CommandRecord {
            seq: 0,
            source: Source::Agent,
            command: spec.name.to_string(),
            params: input,
            ok: result.is_ok(),
            error: result.as_ref().err().cloned(),
            mutates: spec.mutates,
            at: Default::default(),
            created: result.as_ref().map(nori_control::registry::created_layers).unwrap_or_default(),
            result: None,
            checkpoint: None,
        });
        let (mut output, is_error) = tool_output(&result);
        let mut pictures = vec![];
        if let Ok(v) = &result {
            for path in nori_control::vision::pictures_in(spec.name, v) {
                match nori_control::vision::picture(&path).await {
                    Ok(p) => pictures.push(p),
                    Err(e) => output.push_str(&format!("\n(The picture couldn't be shown: {e})")),
                }
            }
        }
        self.command(record, result.ok());
        Ran { output, is_error, pictures }
    }
}
