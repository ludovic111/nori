//! What can run the agent, as data: for each provider its group, label, wire protocol and
//! quirks, default model and address, where its key lives and where to get one, and a short
//! list of models for when its own list can't be fetched.
//!
//! The ids are `nori_control::settings::AGENT_PROVIDERS`; [`ProviderKind`] has one variant per
//! id. Keys are the ids of `nori_control::commands::agent::AGENT_KEY_IDS` (`agent.setKey`).

use serde::Serialize;

use crate::ProviderKind;

/// How a provider is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    /// lsuite AI: the subscription, signed in with the lsuite account.
    Lsuite,
    /// A coding CLI installed on this computer, with `nori-mcp --live` attached.
    Cli,
    /// A model API, with a key.
    Api,
    /// A model server on this computer or the local network.
    Local,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Lsuite, Group::Cli, Group::Api, Group::Local];

    pub fn label(self) -> &'static str {
        match self {
            Group::Lsuite => "lsuite AI",
            Group::Cli => "On this computer",
            Group::Api => "Model APIs",
            Group::Local => "Local servers",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Group::Lsuite => "lsuite",
            Group::Cli => "cli",
            Group::Api => "api",
            Group::Local => "local",
        }
    }
}

/// The request format a provider speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Cli,
    /// Anthropic's Messages API (also lsuite AI's).
    Anthropic,
    /// Chat Completions, with the provider's quirks.
    Chat(Quirks),
    Ollama,
}

/// Where Chat Completions servers differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quirks {
    /// Ask for token counts in the stream (`stream_options.include_usage`).
    pub usage: bool,
    /// At most this many tools in one request.
    pub tool_limit: Option<usize>,
    /// Only a short list of tools (small local models).
    pub compact: bool,
}

impl Quirks {
    pub const STANDARD: Quirks = Quirks { usage: true, tool_limit: None, compact: false };
}

/// Where a provider's API key comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpec {
    /// Keychain id (`agent.setKey`'s provider).
    pub id: &'static str,
    /// Environment variables read when nothing is saved, in order.
    pub env: &'static [&'static str],
    /// A key is needed (else optional: a local server may ask for one).
    pub required: bool,
    /// Where people make one.
    pub url: Option<&'static str>,
    /// What one looks like, for the field's placeholder.
    pub hint: &'static str,
}

/// Everything static about a provider.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub kind: ProviderKind,
    pub id: &'static str,
    pub label: &'static str,
    pub group: Group,
    /// One plain line on what it is.
    pub tagline: &'static str,
    pub wire: Wire,
    /// Model used when none is chosen (empty: the CLI's or server's own).
    pub default_model: &'static str,
    /// Empty for the CLIs, custom servers (the person gives it) and lsuite AI (the account's
    /// server, `<server>/api/ai`).
    pub default_base_url: &'static str,
    /// The address is the person's to give (a custom server).
    pub needs_base_url: bool,
    /// What goes in the address field.
    pub base_url_hint: &'static str,
    pub key: Option<KeySpec>,
    /// Models shown when the list can't be fetched, the default first.
    pub models: &'static [&'static str],
    pub website: &'static str,
}

const fn key(id: &'static str, env: &'static [&'static str], url: &'static str, hint: &'static str) -> Option<KeySpec> {
    Some(KeySpec { id, env, required: true, url: Some(url), hint })
}

pub const ALL: &[Info] = &[
    // ---- lsuite AI ----
    Info {
        kind: ProviderKind::Lsuite,
        id: "lsuite",
        label: "lsuite AI",
        group: Group::Lsuite,
        tagline: "No setup. Sign in and your agent works.",
        wire: Wire::Anthropic,
        default_model: "claude-sonnet-5-5",
        default_base_url: "",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &["claude-sonnet-5-5", "claude-haiku-4-5", "claude-opus-5-5"],
        website: "https://lsuite.xyz/ai",
    },
    // ---- on this computer ----
    Info {
        kind: ProviderKind::ClaudeCode,
        id: "claude-code",
        label: "Claude Code",
        group: Group::Cli,
        tagline: "Your Claude subscription, through the Claude Code app.",
        wire: Wire::Cli,
        default_model: "",
        default_base_url: "",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &["opus", "sonnet", "haiku"],
        website: "https://claude.com/claude-code",
    },
    Info {
        kind: ProviderKind::Codex,
        id: "codex",
        label: "Codex",
        group: Group::Cli,
        tagline: "Your ChatGPT plan, through OpenAI's Codex CLI.",
        wire: Wire::Cli,
        default_model: "",
        default_base_url: "",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &[],
        website: "https://developers.openai.com/codex/cli",
    },
    // ---- model APIs ----
    Info {
        kind: ProviderKind::Anthropic,
        id: "anthropic",
        label: "Anthropic API",
        group: Group::Api,
        tagline: "Claude models, billed per use by Anthropic.",
        wire: Wire::Anthropic,
        default_model: "claude-sonnet-5-5",
        default_base_url: "https://api.anthropic.com",
        needs_base_url: false,
        base_url_hint: "",
        key: key("anthropic", &["ANTHROPIC_API_KEY"], "https://platform.claude.com/settings/keys", "sk-ant-…"),
        models: &["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-4-5", "claude-fable-5-1"],
        website: "https://www.anthropic.com/api",
    },
    Info {
        kind: ProviderKind::OpenAi,
        id: "openai",
        label: "OpenAI API",
        group: Group::Api,
        tagline: "GPT models, billed per use by OpenAI.",
        // OpenAI takes at most 128 functions in one request.
        wire: Wire::Chat(Quirks { tool_limit: Some(128), ..Quirks::STANDARD }),
        default_model: "gpt-5",
        default_base_url: "https://api.openai.com/v1",
        needs_base_url: false,
        base_url_hint: "",
        key: key("openai", &["OPENAI_API_KEY"], "https://platform.openai.com/api-keys", "sk-…"),
        models: &["gpt-5"],
        website: "https://platform.openai.com",
    },
    // ---- local servers ----
    Info {
        kind: ProviderKind::Ollama,
        id: "ollama",
        label: "Ollama",
        group: Group::Local,
        tagline: "Models on this computer. Nothing leaves it.",
        wire: Wire::Ollama,
        default_model: "",
        default_base_url: "http://127.0.0.1:11434",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &[],
        website: "https://ollama.com",
    },
    Info {
        kind: ProviderKind::OpenAiCompatible,
        id: "openai-compatible",
        label: "OpenAI-compatible server",
        group: Group::Local,
        tagline: "Any server that speaks OpenAI's Chat Completions: LM Studio, vLLM, llama.cpp, LiteLLM, a proxy…",
        wire: Wire::Chat(Quirks { usage: false, ..Quirks::STANDARD }),
        default_model: "",
        default_base_url: "",
        needs_base_url: true,
        base_url_hint: "The server's address, e.g. http://127.0.0.1:1234/v1",
        key: Some(KeySpec { id: "openai-compatible", env: &["OPENAI_COMPATIBLE_API_KEY"], required: false, url: None, hint: "Only if the server asks for one" }),
        models: &[],
        website: "",
    },
];

/// The static facts about `kind`.
pub fn info(kind: ProviderKind) -> &'static Info {
    ALL.iter().find(|i| i.kind == kind).expect("every provider is in providers::ALL")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_the_settings_ids() {
        let ids: Vec<&str> = ALL.iter().map(|i| i.id).collect();
        assert_eq!(ids, nori_control::settings::AGENT_PROVIDERS);
        let kinds: Vec<ProviderKind> = ALL.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, ProviderKind::ALL);
        for i in ALL {
            assert_eq!(i.kind.id(), i.id);
            assert_eq!(ProviderKind::parse(i.id), Some(i.kind));
            assert_eq!(i.group == Group::Cli, i.wire == Wire::Cli, "{}", i.id);
            if let Some(k) = i.key.filter(|k| k.required) {
                assert!(k.url.is_some(), "{}: a key is needed, so say where to get one", i.id);
            }
            if !i.default_model.is_empty() && !i.models.is_empty() {
                assert_eq!(i.models[0], i.default_model, "{}: the default model comes first", i.id);
            }
        }
    }
}
