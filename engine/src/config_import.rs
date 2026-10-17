//! One-click competitor config import (China research steal #5).
//!
//! Detects AI coding tools already installed on this machine and imports
//! their configuration into ZenCode: model preferences, custom skills,
//! MCP servers, slash commands, and rules/memories.
//!
//! Supported competitors: Claude Code, Codex CLI, Cursor, Continue,
//! Cline, Roo Code, Aider, OpenCode.
//!
//! # Security invariants (all covered by tests)
//!
//! - **API keys are never copied.** Key-looking values are detected, listed
//!   by name and source file, and the user pastes them into ZenCode's own
//!   secure store (TokenStore, PR #66) manually. There is no code path that
//!   writes a detected secret anywhere.
//! - Imported skills land **UNTRUSTED** (Project-equivalent posture per
//!   PR #73's [`SkillTrust`] model): the user approves each before first use.
//! - Imported MCP servers land **Untrusted** (per PR #77's
//!   [`McpServerTrust`]): every tool call asks until approved.
//! - [`plan`] is a pure dry-run: it only reads. [`apply`] writes only items
//!   the user left selected, never overwrites existing ZenCode files, and
//!   merges MCP JSON instead of replacing it.
//!
//! # Integration seams (sibling PRs, unmerged at time of writing)
//!
//! - Model preferences are mapped to PR #66's [`ProviderKind`] vocabulary
//!   (`OpenAICompatible { base_url }` / `Anthropic { base_url }` / Ollama
//!   preset) and written to `~/.zencode/imported-providers.json` as
//!   *suggestions* — the user activates one in Settings. When PR #66 merges,
//!   replace [`MappedProvider`] with `ProviderKind`.
//! - Skills go to `~/.zencode/skills/` with Project-equivalent trust; when
//!   PR #73 merges, register the import dir accordingly (never
//!   `SkillTrust::User`).
//! - MCP servers merge into `~/.zencode/mcp.json` marked untrusted; when
//!   PR #77 merges, hand the merged map to its config loader.
//! - Slash-command wiring: add `/import` to `slash.rs` (PR #76) as a thin
//!   wrapper around [`plan`] + [`apply`]; see `docs/config-import.md`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Competitors
// ---------------------------------------------------------------------------

/// A competitor tool we can import configuration from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Competitor {
    ClaudeCode,
    Codex,
    Cursor,
    Continue,
    Cline,
    RooCode,
    Aider,
    OpenCode,
}

impl Competitor {
    pub fn all() -> Vec<Competitor> {
        vec![
            Competitor::ClaudeCode,
            Competitor::Codex,
            Competitor::Cursor,
            Competitor::Continue,
            Competitor::Cline,
            Competitor::RooCode,
            Competitor::Aider,
            Competitor::OpenCode,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Competitor::ClaudeCode => "Claude Code",
            Competitor::Codex => "Codex CLI",
            Competitor::Cursor => "Cursor",
            Competitor::Continue => "Continue",
            Competitor::Cline => "Cline",
            Competitor::RooCode => "Roo Code",
            Competitor::Aider => "Aider",
            Competitor::OpenCode => "OpenCode",
        }
    }

    /// Config roots to probe, in priority order. The first existing one wins
    /// for detection; all existing ones are scanned during planning.
    fn roots(&self, home: &Path) -> Vec<PathBuf> {
        match self {
            Competitor::ClaudeCode => vec![home.join(".claude"), home.join(".claude.json")],
            Competitor::Codex => vec![home.join(".codex")],
            Competitor::Cursor => vec![home.join(".cursor")],
            Competitor::Continue => vec![home.join(".continue")],
            Competitor::Cline => vec![
                home.join(".config/Code/User/globalStorage/saoudrizwan.claude-dev"),
                home.join("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev"),
            ],
            Competitor::RooCode => vec![
                home.join(".config/Code/User/globalStorage/rooveterinaryinc.roo-cline"),
                home.join("Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline"),
            ],
            Competitor::Aider => vec![home.join(".aider.conf.yml"), home.join(".aider.model.settings.yml")],
            Competitor::OpenCode => vec![home.join(".config/opencode")],
        }
    }
}

/// One detected competitor installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedInstall {
    pub competitor: Competitor,
    /// The config root that exists (directory or file).
    pub root: PathBuf,
}

/// Detect which competitor tools have configuration on this machine.
///
/// `home` is the user's home directory (passed explicitly so tests can use
/// fixtures; production passes the real home dir).
pub fn detect(home: &Path) -> Vec<DetectedInstall> {
    let mut out = Vec::new();
    for competitor in Competitor::all() {
        for root in competitor.roots(home) {
            if root.exists() {
                out.push(DetectedInstall { competitor, root });
                break;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Provider mapping (seam for PR #66 `ProviderKind`)
// ---------------------------------------------------------------------------

/// Where a competitor's model string should point in ZenCode terms.
///
/// Mirrors PR #66's `ProviderKind` vocabulary so the mapping can be swapped
/// mechanically when that PR merges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappedProvider {
    /// Native Anthropic Messages API (`POST {base_url}/v1/messages`).
    Anthropic { base_url: String },
    /// Any OpenAI-compatible chat-completions endpoint.
    OpenAiCompatible { base_url: String, label: String },
    /// Local Ollama (`http://localhost:11434/v1`).
    Ollama,
    /// Could not be mapped automatically; the raw string is kept so the
    /// user can map it by hand in Settings.
    Unknown,
}

/// A competitor model preference translated to ZenCode terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMapping {
    /// The raw model string from the competitor config, e.g. `"gpt-5"`.
    pub original: String,
    pub provider: MappedProvider,
    /// Suggested display name, e.g. `"openai/gpt-5"`.
    pub suggested_name: String,
}

/// Map a competitor model/provider string to a ZenCode provider.
///
/// Accepts `"provider/model"`, bare model names (`"gpt-5"`), and provider
/// names (`"anthropic"`). Matching is case-insensitive and never panics.
pub fn map_model(raw: &str) -> ModelMapping {
    let raw = raw.trim().trim_matches('"').trim_matches('\'');
    let lower = raw.to_lowercase();
    // Split "provider/model" if present.
    let (prefix, model) = match lower.split_once('/') {
        Some((p, m)) => (p, m),
        None => ("", lower.as_str()),
    };

    // Provider-prefix table. Order matters: check the explicit prefix first,
    // then fall back to model-name heuristics.
    let provider = match prefix {
        "anthropic" => MappedProvider::Anthropic {
            base_url: "https://api.anthropic.com".to_string(),
        },
        "openai" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.openai.com/v1".to_string(),
            label: "openai".to_string(),
        },
        "ollama" => MappedProvider::Ollama,
        "openrouter" => MappedProvider::OpenAiCompatible {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            label: "openrouter".to_string(),
        },
        "deepseek" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.deepseek.com/v1".to_string(),
            label: "deepseek".to_string(),
        },
        "gemini" | "google" => MappedProvider::OpenAiCompatible {
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
            label: "gemini".to_string(),
        },
        "xai" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.x.ai/v1".to_string(),
            label: "xai".to_string(),
        },
        "groq" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.groq.com/openai/v1".to_string(),
            label: "groq".to_string(),
        },
        "mistral" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.mistral.ai/v1".to_string(),
            label: "mistral".to_string(),
        },
        "moonshot" | "kimi" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.moonshot.ai/v1".to_string(),
            label: "moonshot".to_string(),
        },
        "zhipu" | "glm" => MappedProvider::OpenAiCompatible {
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            label: "zhipu".to_string(),
        },
        "qwen" | "dashscope" | "alibaba" => MappedProvider::OpenAiCompatible {
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            label: "qwen".to_string(),
        },
        _ => {
            // Heuristics on the bare model name.
            if model.starts_with("claude") {
                MappedProvider::Anthropic {
                    base_url: "https://api.anthropic.com".to_string(),
                }
            } else if model.starts_with("gpt") || model.starts_with("o1") || model.starts_with("o3")
            {
                MappedProvider::OpenAiCompatible {
                    base_url: "https://api.openai.com/v1".to_string(),
                    label: "openai".to_string(),
                }
            } else if model.starts_with("llama")
                || model.starts_with("qwen")
                || model.contains("ollama")
            {
                MappedProvider::Ollama
            } else if model.starts_with("gemini") || model.starts_with("gemma") {
                MappedProvider::OpenAiCompatible {
                    base_url: "https://generativelanguage.googleapis.com/v1beta/openai/"
                        .to_string(),
                    label: "gemini".to_string(),
                }
            } else if model.starts_with("grok") {
                MappedProvider::OpenAiCompatible {
                    base_url: "https://api.x.ai/v1".to_string(),
                    label: "xai".to_string(),
                }
            } else if model.starts_with("deepseek") {
                MappedProvider::OpenAiCompatible {
                    base_url: "https://api.deepseek.com/v1".to_string(),
                    label: "deepseek".to_string(),
                }
            } else if model.starts_with("mistral") || model.starts_with("mixtral") {
                MappedProvider::OpenAiCompatible {
                    base_url: "https://api.mistral.ai/v1".to_string(),
                    label: "mistral".to_string(),
                }
            } else {
                MappedProvider::Unknown
            }
        }
    };

    let suggested_name = match &provider {
        MappedProvider::Anthropic { .. } => format!("anthropic/{}", model_name_part(raw)),
        MappedProvider::OpenAiCompatible { label, .. } => {
            format!("{}/{}", label, model_name_part(raw))
        }
        MappedProvider::Ollama => format!("ollama/{}", model_name_part(raw)),
        MappedProvider::Unknown => raw.to_string(),
    };

    ModelMapping {
        original: raw.to_string(),
        provider,
        suggested_name,
    }
}

/// Map a provider base URL back to a ZenCode provider.
///
/// Used when a competitor config names a custom provider section: if its
/// base URL is a known provider endpoint we map it properly instead of
/// giving up.
fn map_base_url(url: &str) -> Option<MappedProvider> {
    let url = url.trim().trim_end_matches('/');
    Some(match url {
        "https://api.anthropic.com" => MappedProvider::Anthropic {
            base_url: "https://api.anthropic.com".to_string(),
        },
        "https://api.openai.com/v1" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.openai.com/v1".to_string(),
            label: "openai".to_string(),
        },
        "https://openrouter.ai/api/v1" => MappedProvider::OpenAiCompatible {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            label: "openrouter".to_string(),
        },
        "https://api.deepseek.com/v1" => MappedProvider::OpenAiCompatible {
            base_url: "https://api.deepseek.com/v1".to_string(),
            label: "deepseek".to_string(),
        },
        "https://generativelanguage.googleapis.com/v1beta/openai" => {
            MappedProvider::OpenAiCompatible {
                base_url: "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
                label: "gemini".to_string(),
            }
        }
        "http://localhost:11434/v1" | "http://localhost:11434" => MappedProvider::Ollama,
        _ => return None,
    })
}

/// The model part of `"provider/model"`, or the whole string.
/// Keeps everything after the first `/` so nested ids like
/// `openrouter/deepseek/deepseek-v4` survive intact.
fn model_name_part(raw: &str) -> &str {
    match raw.split_once('/') {
        Some((_, rest)) => rest,
        None => raw,
    }
}

// ---------------------------------------------------------------------------
// Secret detection — keys are reported, never copied
// ---------------------------------------------------------------------------

/// A detected secret that the importer refuses to copy.
///
/// The user pastes these into ZenCode's own secure store (TokenStore,
//  PR #66) manually. There is deliberately no API to "import" one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFinding {
    /// Config key name, e.g. `"apiKey"`.
    pub name: String,
    /// File it was found in.
    pub source: PathBuf,
    pub competitor: Competitor,
    /// Human guidance, e.g. which model it belonged to.
    pub hint: String,
}

/// Key names that almost certainly hold secrets (case-insensitive substring).
fn looks_like_secret_key(key: &str) -> bool {
    let k = key.to_lowercase();
    [
        "apikey",
        "api_key",
        "api-key",
        "accesstoken",
        "access_token",
        "secrettoken",
        "secret_token",
        "auth",
        "bearer",
        "password",
        "passwd",
        "clientsecret",
        "client_secret",
    ]
    .iter()
    .any(|pat| k.contains(pat))
        // "token" alone is too broad ("tokenizer"), but as a full key or
        // suffix/prefix it is a secret.
        || k == "token"
        || k.ends_with("_token")
        || k.ends_with("-token")
        || k.starts_with("token_")
        || k.starts_with("token-")
}

/// Value shapes that look like real credentials.
fn looks_like_secret_value(value: &str) -> bool {
    let v = value.trim().trim_matches('"').trim_matches('\'');
    if v.is_empty() || v.len() < 8 {
        return false;
    }
    // Obviously fake / placeholder values are not secrets.
    let lower = v.to_lowercase();
    if lower.contains("fake")
        || lower.contains("placeholder")
        || lower.contains("example")
        || lower.contains("your-")
        || lower.contains("xxx")
        || lower == "null"
        || lower == "none"
    {
        return false;
    }
    let prefixes = [
        "sk-",
        "xoxb-",
        "xoxp-",
        "ghp_",
        "gho_",
        "github_pat_",
        "glpat-",
        "AKIA",
        "AIza",
        "Bearer ",
        "bearer ",
    ];
    if prefixes.iter().any(|p| v.starts_with(p)) {
        return true;
    }
    // Long opaque strings (hex/base64-ish, no spaces) are suspicious —
    // but not model ids like "anthropic/claude-opus-4-6" (real keys never
    // contain '/').
    v.len() >= 24
        && !v.contains(' ')
        && !v.contains('/')
        && v.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '=' || c == '+'
        })
}

/// Returns true if this key/value pair should be treated as a secret and
/// therefore refused for import.
pub fn is_secret_pair(key: &str, value: &str) -> bool {
    looks_like_secret_key(key) || looks_like_secret_value(value)
}

// ---------------------------------------------------------------------------
// Import plan (dry-run)
// ---------------------------------------------------------------------------

/// What kind of thing an import item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportKind {
    /// A model preference → provider mapping suggestion.
    Model,
    /// A skill directory (must contain SKILL.md) → `~/.zencode/skills/`.
    Skill,
    /// An MCP server definition → `~/.zencode/mcp.json` (Untrusted).
    McpServer,
    /// A slash-command markdown file → `~/.zencode/commands/`.
    Command,
    /// A rules/memory file (CLAUDE.md, AGENTS.md, …) → `~/.zencode/imported/`.
    Rule,
}

impl ImportKind {
    fn display(&self) -> &'static str {
        match self {
            ImportKind::Model => "model",
            ImportKind::Skill => "skill",
            ImportKind::McpServer => "mcp server",
            ImportKind::Command => "command",
            ImportKind::Rule => "rule",
        }
    }
}

/// One importable thing, shown to the user as a checkbox row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportItem {
    /// Stable id, e.g. `"claude-code:skill:pdf"`.
    pub id: String,
    pub kind: ImportKind,
    pub competitor: Competitor,
    /// Where it comes from.
    pub source: PathBuf,
    /// Short human label.
    pub label: String,
    /// What will happen, in one line.
    pub detail: String,
    /// Where it will land.
    pub destination: PathBuf,
    /// Checkbox state. Defaults to true; pre-unchecked when the destination
    /// already exists (would be skipped anyway).
    pub selected: bool,
    /// Model mapping, for `ImportKind::Model` items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_mapping: Option<ModelMapping>,
}

/// The full dry-run result of scanning a machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPlan {
    pub detected: Vec<DetectedInstall>,
    pub items: Vec<ImportItem>,
    /// Secrets found but refused for import.
    pub secrets: Vec<SecretFinding>,
    /// Non-fatal notes (unparseable files, unknown models, …).
    pub warnings: Vec<String>,
}

impl ImportPlan {
    pub fn selected_items(&self) -> Vec<&ImportItem> {
        self.items.iter().filter(|i| i.selected).collect()
    }
}

// ---------------------------------------------------------------------------
// Tiny scalar helpers (no new dependencies)
// ---------------------------------------------------------------------------

/// Strip surrounding quotes and whitespace from a scalar.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Read a JSON file, returning None when missing or unparseable (with a
/// warning pushed by the caller).
fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Minimal TOML reader for the flat keys this importer needs.
///
/// Supports `key = "value"` lines and `[section]` / `[a.b.c]` headers.
/// Returns `(section_path, key, value)` triples. Not a general TOML parser;
/// complex Codex configs degrade to warnings, never to wrong imports.
fn read_toml_flat(path: &Path) -> Option<Vec<(String, String, String)>> {
    let text = fs::read_to_string(path).ok()?;
    let mut section = String::new();
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim().to_string();
            // Drop trailing comments outside quotes (best effort).
            let mut val = v.trim().to_string();
            if let Some(hash) = val.find(" #") {
                val.truncate(hash);
            }
            out.push((section.clone(), key, unquote(val.trim())));
        }
    }
    Some(out)
}

/// Extract top-level `key: value` pairs from a YAML-ish file.
///
/// Indentation-aware only to the extent this importer needs: top-level keys
/// (indent 0) and one nesting level for list items are handled by the
/// callers. Not a general YAML parser.
fn yaml_top_level(path: &Path, keys: &[&str]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return out;
    };
    for raw_line in text.lines() {
        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            continue;
        }
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if keys.contains(&k) {
                out.insert(k.to_string(), unquote(v));
            }
        }
    }
    out
}

/// Push a secret finding for a key/value pair.
fn secret(
    secrets: &mut Vec<SecretFinding>,
    competitor: Competitor,
    source: &Path,
    name: &str,
    hint: &str,
) {
    secrets.push(SecretFinding {
        name: name.to_string(),
        source: source.to_path_buf(),
        competitor,
        hint: hint.to_string(),
    });
}

fn item_id(competitor: Competitor, kind: ImportKind, label: &str) -> String {
    let slug: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "{}:{}:{}",
        format!("{:?}", competitor)
            .to_lowercase()
            .replace("claudecode", "claude-code")
            .replace("roocode", "roo-code"),
        kind.display().replace(' ', "-"),
        slug
    )
}

// ---------------------------------------------------------------------------
// Per-competitor collectors
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    home: &'a Path,
    zhome: &'a Path,
    items: Vec<ImportItem>,
    secrets: Vec<SecretFinding>,
    warnings: Vec<String>,
}

impl<'a> Ctx<'a> {
    fn push(
        &mut self,
        competitor: Competitor,
        kind: ImportKind,
        source: PathBuf,
        label: String,
        detail: String,
        destination: PathBuf,
    ) {
        let id = item_id(competitor, kind, &label);
        // MCP servers and model suggestions merge into shared files, so an
        // existing destination is not a conflict — only single-file copies
        // (skills, commands, rules) can collide.
        let merge_safe = matches!(kind, ImportKind::McpServer | ImportKind::Model);
        let exists = !merge_safe && destination.exists();
        if exists {
            self.warnings.push(format!(
                "{}: '{}' already exists at {} — will skip",
                competitor.display_name(),
                label,
                destination.display()
            ));
        }
        self.items.push(ImportItem {
            id,
            kind,
            competitor,
            source,
            label,
            detail,
            destination,
            selected: !exists,
            model_mapping: None,
        });
    }

    fn push_model(
        &mut self,
        competitor: Competitor,
        source: PathBuf,
        label: String,
        raw_model: &str,
    ) {
        let mapping = map_model(raw_model);
        let detail = match &mapping.provider {
            MappedProvider::Unknown => format!(
                "model '{}' → could not map automatically; kept for manual mapping",
                mapping.original
            ),
            _ => format!("model '{}' → {}", mapping.original, mapping.suggested_name),
        };
        let dest = self.zhome.join("imported-providers.json");
        let id = item_id(competitor, ImportKind::Model, &label);
        self.items.push(ImportItem {
            id,
            kind: ImportKind::Model,
            competitor,
            source,
            label,
            detail,
            destination: dest,
            selected: true,
            model_mapping: Some(mapping),
        });
    }
}

/// Collect MCP servers from a JSON value shaped like
/// `{"mcpServers": {name: config}}` or `{"mcp": {name: config}}`.
fn collect_mcp_map(ctx: &mut Ctx, competitor: Competitor, source: &Path, value: &Value) {
    let map = value
        .get("mcpServers")
        .or_else(|| value.get("mcp"))
        .and_then(|v| v.as_object());
    let Some(map) = map else { return };
    for (name, cfg) in map {
        // Refuse to carry secrets inside env blocks: scan and redact.
        if let Some(env) = cfg.get("env").and_then(|e| e.as_object()) {
            for (k, v) in env {
                if let Some(vs) = v.as_str() {
                    if is_secret_pair(k, vs) {
                        secret(
                            &mut ctx.secrets,
                            competitor,
                            source,
                            &format!("mcp env {}.{}", name, k),
                            &format!(
                                "MCP server '{}' declares a secret env var — paste it into ZenCode Settings yourself",
                                name
                            ),
                        );
                    }
                }
            }
        }
        let command = cfg
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("(unknown command)");
        ctx.push(
            competitor,
            ImportKind::McpServer,
            source.to_path_buf(),
            name.clone(),
            format!("{} — imports as Untrusted", command),
            ctx.zhome.join("mcp.json"),
        );
    }
}

/// Import skill directories (must contain SKILL.md) from `dir`.
fn collect_skill_dirs(ctx: &mut Ctx, competitor: Competitor, dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join("SKILL.md").is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unnamed".to_string());
        ctx.push(
            competitor,
            ImportKind::Skill,
            path.clone(),
            name.clone(),
            format!(
                "skill '{}' → ~/.zencode/skills/ (UNTRUSTED until you approve it)",
                name
            ),
            ctx.zhome.join("skills").join(&name),
        );
    }
}

/// Import `*.md` slash commands from `dir` (recursive, one level).
fn collect_commands(ctx: &mut Ctx, competitor: Competitor, dir: &Path) {
    fn visit(ctx: &mut Ctx, competitor: Competitor, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(ctx, competitor, &path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ctx.push(
                        competitor,
                        ImportKind::Command,
                        path.clone(),
                        stem.to_string(),
                        format!("command '/{}' from {}", stem, competitor.display_name()),
                        ctx.zhome.join("commands").join(format!("{}.md", stem)),
                    );
                }
            }
        }
    }
    visit(ctx, competitor, dir);
}

/// Import a rules/memory markdown file.
fn collect_rule(ctx: &mut Ctx, competitor: Competitor, path: &Path, label: &str) {
    if !path.is_file() {
        return;
    }
    let dest_name = format!(
        "{}-{}",
        format!("{:?}", competitor).to_lowercase(),
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    ctx.push(
        competitor,
        ImportKind::Rule,
        path.to_path_buf(),
        label.to_string(),
        format!("rules/memory '{}' → ~/.zencode/imported/", label),
        ctx.zhome.join("imported").join(dest_name),
    );
}

// --- Claude Code ------------------------------------------------------------

fn collect_claude_code(ctx: &mut Ctx) {
    let c = Competitor::ClaudeCode;
    // ~/.claude.json — MCP servers live here in some versions.
    let dot_json = ctx.home.join(".claude.json");
    if let Some(v) = read_json(&dot_json) {
        collect_mcp_map(ctx, c, &dot_json, &v);
        if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
            ctx.push_model(c, dot_json.clone(), "default model".into(), model);
        }
    } else if dot_json.exists() {
        ctx.warnings.push(format!(
            "Claude Code: could not parse {} — skipped",
            dot_json.display()
        ));
    }
    let dir = ctx.home.join(".claude");
    if !dir.is_dir() {
        return;
    }
    // settings.json — model + MCP servers.
    let settings = dir.join("settings.json");
    if let Some(v) = read_json(&settings) {
        collect_mcp_map(ctx, c, &settings, &v);
        if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
            ctx.push_model(c, settings.clone(), "default model".into(), model);
        }
        // Scan the whole settings object for secrets (apiKey etc.).
        scan_json_for_secrets(ctx, c, &settings, &v, "settings");
    } else if settings.exists() {
        ctx.warnings.push(format!(
            "Claude Code: could not parse {} — skipped",
            settings.display()
        ));
    }
    collect_skill_dirs(ctx, c, &dir.join("skills"));
    collect_commands(ctx, c, &dir.join("commands"));
    collect_rule(ctx, c, &dir.join("CLAUDE.md"), "global memory (CLAUDE.md)");
}

// --- Codex ------------------------------------------------------------------

fn collect_codex(ctx: &mut Ctx) {
    let c = Competitor::Codex;
    let dir = ctx.home.join(".codex");
    if !dir.is_dir() {
        return;
    }
    let config = dir.join("config.toml");
    if let Some(entries) = read_toml_flat(&config) {
        // Custom provider base_urls, for accurate mapping.
        let mut provider_urls: BTreeMap<String, String> = BTreeMap::new();
        for (section, key, value) in &entries {
            if section.starts_with("model_providers.") && key == "base_url" {
                if let Some(name) = section.strip_prefix("model_providers.") {
                    provider_urls.insert(name.to_string(), value.clone());
                }
            }
            if is_secret_pair(key, value) {
                secret(
                    &mut ctx.secrets,
                    c,
                    &config,
                    key,
                    "Codex config declares a secret — paste it into ZenCode Settings yourself",
                );
            }
        }
        for (section, key, value) in &entries {
            if section.is_empty() && key == "model" {
                let mut mapping = map_model(value);
                // A named `model_provider` section may carry a custom
                // base_url: map it when it's a known endpoint, otherwise
                // keep the model for manual mapping.
                if let Some(provider_name) = entries
                    .iter()
                    .find(|(s, k, _)| s.is_empty() && k == "model_provider")
                    .map(|(_, _, v)| v)
                {
                    match provider_urls
                        .get(provider_name)
                        .and_then(|u| map_base_url(u))
                    {
                        Some(p) => mapping.provider = p,
                        None => {
                            ctx.warnings.push(format!(
                                "Codex: model '{}' uses custom provider '{}' — kept for manual mapping",
                                value,
                                provider_urls.get(provider_name).map(|s| s.as_str()).unwrap_or(provider_name)
                            ));
                            mapping.provider = MappedProvider::Unknown;
                            mapping.suggested_name = value.clone();
                        }
                    }
                }
                let id = item_id(c, ImportKind::Model, "default model");
                ctx.items.push(ImportItem {
                    id,
                    kind: ImportKind::Model,
                    competitor: c,
                    source: config.clone(),
                    label: "default model".to_string(),
                    detail: match &mapping.provider {
                        MappedProvider::Unknown => {
                            format!("model '{}' → kept for manual mapping", mapping.original)
                        }
                        _ => format!("model '{}' → {}", mapping.original, mapping.suggested_name),
                    },
                    destination: ctx.zhome.join("imported-providers.json"),
                    selected: true,
                    model_mapping: Some(mapping),
                });
            }
        }
    } else if config.exists() {
        ctx.warnings.push(format!(
            "Codex: could not parse {} — skipped",
            config.display()
        ));
    }
    // auth.json is secrets-only: report, never read values into items.
    let auth = dir.join("auth.json");
    if auth.exists() {
        secret(
            &mut ctx.secrets,
            c,
            &auth,
            "OPENAI_API_KEY (auth.json)",
            "Codex OAuth/API key — sign in again inside ZenCode Settings; it is never copied",
        );
    }
}

// --- Cursor -----------------------------------------------------------------

fn collect_cursor(ctx: &mut Ctx) {
    let c = Competitor::Cursor;
    let dir = ctx.home.join(".cursor");
    if !dir.is_dir() {
        return;
    }
    let mcp = dir.join("mcp.json");
    if let Some(v) = read_json(&mcp) {
        collect_mcp_map(ctx, c, &mcp, &v);
    } else if mcp.exists() {
        ctx.warnings.push(format!(
            "Cursor: could not parse {} — skipped",
            mcp.display()
        ));
    }
    ctx.warnings.push(
        "Cursor: project rules live in each project's .muse/rules/ — open the project in ZenCode and import from there"
            .to_string(),
    );
}

// --- Continue ---------------------------------------------------------------

fn collect_continue(ctx: &mut Ctx) {
    let dir = ctx.home.join(".continue");
    if !dir.is_dir() {
        return;
    }
    // Newer JSON config first, then legacy YAML.
    let json_cfg = dir.join("config.json");
    if let Some(v) = read_json(&json_cfg) {
        collect_continue_models_json(ctx, &json_cfg, &v);
    }
    let yaml_cfg = dir.join("config.yaml");
    if yaml_cfg.is_file() {
        collect_continue_models_yaml(ctx, &yaml_cfg);
    }
    if !json_cfg.exists() && !yaml_cfg.exists() {
        ctx.warnings
            .push("Continue: no config.json or config.yaml found".to_string());
    }
}

fn collect_continue_models_json(ctx: &mut Ctx, source: &Path, v: &Value) {
    let c = Competitor::Continue;
    let models = v.get("models").and_then(|m| m.as_array());
    let Some(models) = models else { return };
    for m in models {
        let name = m.get("name").and_then(|n| n.as_str()).unwrap_or("model");
        // Secrets first: apiKey must never become an item.
        for key in ["apiKey", "api_key"] {
            if let Some(val) = m.get(key).and_then(|x| x.as_str()) {
                if !val.trim().is_empty() {
                    secret(
                        &mut ctx.secrets,
                        c,
                        source,
                        &format!("{}.{}", name, key),
                        &format!(
                            "Continue model '{}' declares an apiKey — paste it into ZenCode Settings yourself",
                            name
                        ),
                    );
                }
            }
        }
        let provider = m.get("provider").and_then(|p| p.as_str()).unwrap_or("");
        let model = m.get("model").and_then(|x| x.as_str()).unwrap_or("");
        if model.is_empty() {
            continue;
        }
        let raw = if provider.is_empty() {
            model.to_string()
        } else {
            format!("{}/{}", provider, model)
        };
        ctx.push_model(
            c,
            source.to_path_buf(),
            format!("Continue model '{}'", name),
            &raw,
        );
    }
}

/// Shallow scan of Continue's YAML for `models:` list items.
///
/// Handles the common shape:
/// ```yaml
/// models:
///   - name: GPT-4o
///     provider: openai
///     model: gpt-4o
///     apiKey: ...
/// ```
fn collect_continue_models_yaml(ctx: &mut Ctx, source: &Path) {
    let c = Competitor::Continue;
    let Ok(text) = fs::read_to_string(source) else {
        return;
    };
    let mut in_models = false;
    let mut current: BTreeMap<String, String> = BTreeMap::new();

    let flush = |ctx: &mut Ctx, current: &BTreeMap<String, String>| {
        let empty = String::new();
        let model = current.get("model").unwrap_or(&empty);
        if model.is_empty() {
            return;
        }
        let name = current.get("name").map(|s| s.as_str()).unwrap_or("model");
        for key in ["apiKey", "api_key"] {
            if let Some(val) = current.get(key) {
                if !val.trim().is_empty() {
                    secret(
                        &mut ctx.secrets,
                        c,
                        source,
                        &format!("{}.{}", name, key),
                        &format!(
                            "Continue model '{}' declares an apiKey — paste it into ZenCode Settings yourself",
                            name
                        ),
                    );
                }
            }
        }
        let provider = current.get("provider").map(|s| s.as_str()).unwrap_or("");
        let raw = if provider.is_empty() {
            model.clone()
        } else {
            format!("{}/{}", provider, model)
        };
        ctx.push_model(
            c,
            source.to_path_buf(),
            format!("Continue model '{}'", name),
            &raw,
        );
    };

    for raw_line in text.lines() {
        let indent = raw_line.len() - raw_line.trim_start().len();
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if indent == 0 {
            let was_in_models = in_models;
            in_models = line == "models:";
            if was_in_models && !in_models && !current.is_empty() {
                // Left the models section with an unflushed item: flush it
                // rather than dropping it.
                flush(ctx, &current);
                current.clear();
            }
            continue;
        }
        if !in_models {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ") {
            if !current.is_empty() {
                flush(ctx, &current);
                current.clear();
            }
            let rest = rest.trim();
            if let Some((k, v)) = rest.split_once(':') {
                current.insert(k.trim().to_string(), unquote(v));
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            current.insert(k.trim().to_string(), unquote(v));
        }
    }
    if !current.is_empty() {
        flush(ctx, &current);
    }
}

// --- Cline / Roo Code (VS Code global storage) ------------------------------

fn collect_vscode_agent(ctx: &mut Ctx, competitor: Competitor, storage_dir_name: &str) {
    for root in competitor.roots(ctx.home) {
        if !root.is_dir() {
            continue;
        }
        // MCP settings.
        for file in ["cline_mcp_settings.json", "mcp_settings.json"] {
            let p = root.join("settings").join(file);
            if p.is_file() {
                if let Some(v) = read_json(&p) {
                    collect_mcp_map(ctx, competitor, &p, &v);
                } else {
                    ctx.warnings.push(format!(
                        "{}: could not parse {} — skipped",
                        competitor.display_name(),
                        p.display()
                    ));
                }
            }
        }
        // Custom modes are editor-specific; note, don't import.
        let _ = storage_dir_name;
    }
    ctx.warnings.push(format!(
        "{}: custom modes and chat history are editor-specific and are not imported",
        competitor.display_name()
    ));
}

// --- Aider ------------------------------------------------------------------

fn collect_aider(ctx: &mut Ctx) {
    let c = Competitor::Aider;
    for file in [".aider.conf.yml", ".aider.model.settings.yml"] {
        let p = ctx.home.join(file);
        if !p.is_file() {
            continue;
        }
        let pairs = yaml_top_level(
            &p,
            &["model", "weak-model", "editor-model", "api-key", "api_key"],
        );
        for (k, v) in &pairs {
            if is_secret_pair(k, v) {
                secret(
                    &mut ctx.secrets,
                    c,
                    &p,
                    k,
                    "Aider config declares a secret — paste it into ZenCode Settings yourself",
                );
                continue;
            }
        }
        for key in ["model", "weak-model", "editor-model"] {
            if let Some(v) = pairs.get(key) {
                if !v.is_empty() {
                    ctx.push_model(c, p.clone(), format!("aider {}", key), v);
                }
            }
        }
    }
}

// --- OpenCode ----------------------------------------------------------------

fn collect_opencode(ctx: &mut Ctx) {
    let c = Competitor::OpenCode;
    let cfg = ctx.home.join(".config/opencode/opencode.json");
    let Some(v) = read_json(&cfg) else {
        if cfg.exists() {
            ctx.warnings.push(format!(
                "OpenCode: could not parse {} — skipped",
                cfg.display()
            ));
        }
        return;
    };
    if let Some(model) = v.get("model").and_then(|m| m.as_str()) {
        ctx.push_model(c, cfg.clone(), "default model".into(), model);
    }
    collect_mcp_map(ctx, c, &cfg, &v);
    // Provider sections may hold keys (often nested under "options"):
    // report them, never import them.
    if let Some(providers) = v.get("provider").and_then(|p| p.as_object()) {
        for (name, pcfg) in providers {
            scan_value_for_secrets(
                ctx,
                c,
                &cfg,
                pcfg,
                &format!("provider.{}", name),
                &format!("OpenCode provider '{}'", name),
            );
        }
    }
    scan_json_for_secrets(ctx, c, &cfg, &v, "config");
}

/// Recursively report secret-looking key/value pairs under a dotted path.
fn scan_value_for_secrets(
    ctx: &mut Ctx,
    competitor: Competitor,
    source: &Path,
    v: &Value,
    path: &str,
    owner: &str,
) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let child = format!("{}.{}", path, k);
                if let Some(s) = val.as_str() {
                    if is_secret_pair(k, s) {
                        secret(
                            &mut ctx.secrets,
                            competitor,
                            source,
                            &child,
                            &format!(
                                "{} declares a secret ('{}') — paste it into ZenCode Settings yourself",
                                owner, child
                            ),
                        );
                    }
                } else {
                    scan_value_for_secrets(ctx, competitor, source, val, &child, owner);
                }
            }
        }
        Value::Array(arr) => {
            for val in arr {
                scan_value_for_secrets(ctx, competitor, source, val, path, owner);
            }
        }
        _ => {}
    }
}

/// Recursively scan a JSON value for secret-looking key/value pairs.
fn scan_json_for_secrets(
    ctx: &mut Ctx,
    competitor: Competitor,
    source: &Path,
    v: &Value,
    where_: &str,
) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if let Some(s) = val.as_str() {
                    if is_secret_pair(k, s) {
                        secret(
                            &mut ctx.secrets,
                            competitor,
                            source,
                            &format!("{}.{}", where_, k),
                            &format!(
                                "{} declares '{}' — paste it into ZenCode Settings yourself",
                                competitor.display_name(),
                                k
                            ),
                        );
                    }
                } else {
                    scan_json_for_secrets(ctx, competitor, source, val, where_);
                }
            }
        }
        Value::Array(arr) => {
            for val in arr {
                scan_json_for_secrets(ctx, competitor, source, val, where_);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// plan() — the dry run
// ---------------------------------------------------------------------------

/// Scan the machine and build the full dry-run plan.
///
/// Pure read-only: inspects competitor configs under `home` and describes
/// what would land under `zhome` (`~/.zencode`). Nothing is written.
pub fn plan(home: &Path, zhome: &Path) -> Result<ImportPlan> {
    let detected = detect(home);
    let mut ctx = Ctx {
        home,
        zhome,
        items: Vec::new(),
        secrets: Vec::new(),
        warnings: Vec::new(),
    };
    for d in &detected {
        match d.competitor {
            Competitor::ClaudeCode => collect_claude_code(&mut ctx),
            Competitor::Codex => collect_codex(&mut ctx),
            Competitor::Cursor => collect_cursor(&mut ctx),
            Competitor::Continue => collect_continue(&mut ctx),
            Competitor::Cline => collect_vscode_agent(&mut ctx, Competitor::Cline, "cline"),
            Competitor::RooCode => collect_vscode_agent(&mut ctx, Competitor::RooCode, "roo"),
            Competitor::Aider => collect_aider(&mut ctx),
            Competitor::OpenCode => collect_opencode(&mut ctx),
        }
    }
    if detected.is_empty() {
        ctx.warnings.push(
            "No competitor tools detected — nothing to import. Supported: Claude Code, Codex CLI, Cursor, Continue, Cline, Roo Code, Aider, OpenCode.".to_string(),
        );
    }
    // Deterministic order for stable UI rendering and tests.
    ctx.items.sort_by(|a, b| a.id.cmp(&b.id));
    // Deduplicate secrets (same name+source reported by multiple scanners).
    ctx.secrets
        .sort_by(|a, b| (&a.name, &a.source).cmp(&(&b.name, &b.source)));
    ctx.secrets
        .dedup_by(|a, b| a.name == b.name && a.source == b.source);
    Ok(ImportPlan {
        detected,
        items: ctx.items,
        secrets: ctx.secrets,
        warnings: ctx.warnings,
    })
}

// ---------------------------------------------------------------------------
// apply() — execute the user's selection
// ---------------------------------------------------------------------------

/// One item that was actually imported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedItem {
    pub id: String,
    pub kind: ImportKind,
    pub label: String,
    pub destination: PathBuf,
    /// Trust posture applied (for skills / MCP servers).
    pub trust: String,
}

/// The result of applying a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub imported: Vec<ImportedItem>,
    /// Items skipped (deselected, destination existed, …) with reasons.
    pub skipped: Vec<String>,
    /// Secrets that were reported (never copied).
    pub secrets: Vec<SecretFinding>,
}

/// Execute the selected items of a plan.
///
/// - Only items with `selected == true` are touched.
/// - Existing destination files are never overwritten (skipped with a note).
/// - MCP servers are merged into `~/.zencode/mcp.json`, never replacing it.
pub fn apply(plan: &ImportPlan, zhome: &Path) -> Result<ImportReport> {
    let mut report = ImportReport {
        imported: Vec::new(),
        skipped: Vec::new(),
        secrets: plan.secrets.clone(),
    };
    // Group MCP servers so the JSON file is written once.
    let mut mcp_servers: BTreeMap<String, (Value, String)> = BTreeMap::new();
    let mut model_mappings: Vec<(String, ModelMapping)> = Vec::new();

    for item in plan.selected_items() {
        // MCP servers and model suggestions merge into shared files, so an
        // existing destination must not skip them — the merge logic below
        // never overwrites existing entries.
        let merge_safe = matches!(item.kind, ImportKind::McpServer | ImportKind::Model);
        if !merge_safe && item.destination.exists() {
            report.skipped.push(format!(
                "{}: destination {} already exists — skipped (your file wins)",
                item.label,
                item.destination.display()
            ));
            continue;
        }
        match item.kind {
            ImportKind::Skill | ImportKind::Command | ImportKind::Rule => {
                if let Some(parent) = item.destination.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))?;
                }
                if item.kind == ImportKind::Skill {
                    copy_dir(&item.source, &item.destination)
                        .with_context(|| format!("copying skill {}", item.source.display()))?;
                } else {
                    fs::copy(&item.source, &item.destination)
                        .with_context(|| format!("copying {}", item.source.display()))?;
                }
                report.imported.push(ImportedItem {
                    id: item.id.clone(),
                    kind: item.kind,
                    label: item.label.clone(),
                    destination: item.destination.clone(),
                    trust: match item.kind {
                        ImportKind::Skill => "UNTRUSTED (approve before first use)".to_string(),
                        _ => "n/a".to_string(),
                    },
                });
            }
            ImportKind::McpServer => {
                // Re-read the raw server config from the source file.
                if let Some(cfg) = mcp_server_config(&item.source, &item.label) {
                    mcp_servers.insert(
                        namespaced_server_name(item.competitor, &item.label),
                        (cfg, item.competitor.display_name().to_string()),
                    );
                } else {
                    report.skipped.push(format!(
                        "mcp server '{}': could not re-read config — skipped",
                        item.label
                    ));
                }
            }
            ImportKind::Model => {
                if let Some(m) = &item.model_mapping {
                    model_mappings.push((item.label.clone(), m.clone()));
                }
            }
        }
    }

    if !mcp_servers.is_empty() {
        let dest = zhome.join("mcp.json");
        let added = merge_mcp_json(&dest, mcp_servers)?;
        if !added.is_empty() {
            report.imported.push(ImportedItem {
                id: "mcp.json".to_string(),
                kind: ImportKind::McpServer,
                label: format!("{} MCP server(s) merged: {}", added.len(), added.join(", ")),
                destination: dest,
                trust: "Untrusted (every tool call asks until approved)".to_string(),
            });
        }
    }

    if !model_mappings.is_empty() {
        let dest = zhome.join("imported-providers.json");
        if !dest.exists() {
            fs::create_dir_all(zhome).with_context(|| format!("creating {}", zhome.display()))?;
            let suggestions: Vec<Value> = model_mappings
                .iter()
                .map(|(label, m)| {
                    serde_json::json!({
                        "label": label,
                        "original": m.original,
                        "suggested_name": m.suggested_name,
                        "provider": m.provider,
                    })
                })
                .collect();
            fs::write(
                &dest,
                serde_json::to_string_pretty(&serde_json::json!({
                    "_note": "Suggestions from competitor config import. Activate one in ZenCode Settings → Providers; paste your own API key there — keys are never imported.",
                    "suggestions": suggestions,
                }))
                .expect("json! macro values always serialize"),
            )?;
            report.imported.push(ImportedItem {
                id: "imported-providers".to_string(),
                kind: ImportKind::Model,
                label: format!("{} model mapping suggestion(s)", model_mappings.len()),
                destination: dest,
                trust: "n/a (suggestions only, nothing activated)".to_string(),
            });
        } else {
            report.skipped.push(
                "imported-providers.json already exists — model suggestions not overwritten; review it in Settings".to_string(),
            );
        }
    }

    Ok(report)
}

/// `competitor-name` prefix avoids collisions when two tools define a server
/// with the same name.
fn namespaced_server_name(competitor: Competitor, name: &str) -> String {
    let prefix = match competitor {
        Competitor::ClaudeCode => "claude-code",
        Competitor::Codex => "codex",
        Competitor::Cursor => "cursor",
        Competitor::Continue => "continue",
        Competitor::Cline => "cline",
        Competitor::RooCode => "roo-code",
        Competitor::Aider => "aider",
        Competitor::OpenCode => "opencode",
    };
    format!("{}-{}", prefix, name)
}

/// Re-read one MCP server's raw config object from its source file.
fn mcp_server_config(source: &Path, name: &str) -> Option<Value> {
    let v = read_json(source)?;
    let map = v
        .get("mcpServers")
        .or_else(|| v.get("mcp"))
        .and_then(|m| m.as_object())?;
    // `name` here is the plain server name (collectors store it unprefixed).
    map.get(name).cloned()
}

/// Merge servers into `~/.zencode/mcp.json`, marking each untrusted.
///
/// Never overwrites an existing server entry. Trust metadata lives under
/// `_zencode.trust` so PR #77's loader can pick it up when it merges.
/// Returns the names that were actually added.
fn merge_mcp_json(dest: &Path, servers: BTreeMap<String, (Value, String)>) -> Result<Vec<String>> {
    let mut root: Value = read_json(dest).unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        anyhow::bail!(
            "{} is not a JSON object — refusing to merge",
            dest.display()
        );
    }
    // Collect (name, config, trust-entry) first, then apply, so the two
    // mutable borrows of `root` never overlap.
    let mut staged: Vec<(String, Value, Value)> = Vec::new();
    {
        let obj = root.as_object_mut().expect("root verified as object above");
        let mcp = obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        let mcp_obj = mcp
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?;
        for (name, (mut cfg, from)) in servers {
            if mcp_obj.contains_key(&name) {
                continue; // existing entry wins
            }
            // Strip secret env values before writing: defense in depth — the
            // planner already reported them, but a hand-edited plan could slip.
            if let Some(env) = cfg.get_mut("env").and_then(|e| e.as_object_mut()) {
                let secret_keys: Vec<String> = env
                    .iter()
                    .filter(|(k, v)| v.as_str().map(|s| is_secret_pair(k, s)).unwrap_or(false))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in secret_keys {
                    env.remove(&k);
                }
            }
            let trust_entry = serde_json::json!({"trust": "untrusted", "imported_from": from});
            staged.push((name, cfg, trust_entry));
        }
    }
    let mut added = Vec::new();
    let mut trust_entries: Vec<(String, Value)> = Vec::new();
    {
        let obj = root.as_object_mut().expect("root verified as object above");
        let mcp_obj = obj
            .get_mut("mcpServers")
            .and_then(|m| m.as_object_mut())
            .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?;
        for (name, cfg, trust_entry) in staged {
            mcp_obj.insert(name.clone(), cfg);
            trust_entries.push((name.clone(), trust_entry));
            added.push(name);
        }
    }
    {
        let obj = root.as_object_mut().expect("root verified as object above");
        let trust_obj = obj
            .entry("_zencode")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("_zencode is not an object"))?
            .entry("trust")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("_zencode.trust is not an object"))?;
        for (name, trust_entry) in trust_entries {
            trust_obj.insert(name, trust_entry);
        }
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        dest,
        serde_json::to_string_pretty(&root).expect("JSON Value always serializes"),
    )
    .with_context(|| format!("writing {}", dest.display()))?;
    Ok(added)
}

/// Recursive directory copy (skills are small directories).
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Unique scratch dir per test. Values written here are clearly fake.
    fn scratch(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("zci-test-{}-{}-{}", std::process::id(), name, n));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn home_and_zhome(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = scratch(name);
        let home = base.join("home");
        let zhome = base.join("zhome");
        fs::create_dir_all(&home).unwrap();
        (base, home, zhome)
    }

    // -- detection ----------------------------------------------------------

    #[test]
    fn detect_empty_home_finds_nothing() {
        let (_base, home, zhome) = home_and_zhome("empty");
        let plan = plan(&home, &zhome).unwrap();
        assert!(plan.detected.is_empty());
        assert!(plan.items.is_empty());
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("No competitor tools detected")));
        // Dry run must not create anything.
        assert!(!zhome.exists());
    }

    #[test]
    fn detect_finds_each_competitor() {
        let (_base, home, _zhome) = home_and_zhome("detect");
        // One marker per competitor.
        write(&home.join(".claude.json"), "{}");
        write(&home.join(".codex/config.toml"), "");
        write(&home.join(".cursor/mcp.json"), "{}");
        write(&home.join(".continue/config.yaml"), "");
        write(
            &home.join(".config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/x.json"),
            "{}",
        );
        write(
            &home
                .join(".config/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/x.json"),
            "{}",
        );
        write(&home.join(".aider.conf.yml"), "");
        write(&home.join(".config/opencode/opencode.json"), "{}");

        let found = detect(&home);
        assert_eq!(found.len(), 8, "expected all 8, got {:?}", found);
    }

    // -- model mapping ------------------------------------------------------

    #[test]
    fn map_model_table() {
        let cases: &[(&str, &str)] = &[
            ("anthropic/claude-opus-4-6", "anthropic/claude-opus-4-6"),
            ("claude-sonnet-4-6", "anthropic/claude-sonnet-4-6"),
            ("openai/gpt-5", "openai/gpt-5"),
            ("gpt-5", "openai/gpt-5"),
            ("o1-preview", "openai/o1-preview"),
            ("ollama/llama3.1", "ollama/llama3.1"),
            (
                "openrouter/deepseek/deepseek-v4",
                "openrouter/deepseek/deepseek-v4",
            ),
            ("deepseek/deepseek-chat", "deepseek/deepseek-chat"),
            ("gemini/gemini-2.5-pro", "gemini/gemini-2.5-pro"),
            ("xai/grok-4", "xai/grok-4"),
            ("groq/llama-3.3-70b", "groq/llama-3.3-70b"),
            ("mistral/mistral-large", "mistral/mistral-large"),
            ("moonshot/kimi-k2", "moonshot/kimi-k2"),
            ("zhipu/glm-4.6", "zhipu/glm-4.6"),
            ("qwen/qwen3-max", "qwen/qwen3-max"),
        ];
        for (raw, expected) in cases {
            let m = map_model(raw);
            assert_eq!(&m.suggested_name, expected, "raw={}", raw);
            assert!(
                !matches!(m.provider, MappedProvider::Unknown),
                "raw={} should map",
                raw
            );
        }
        let unknown = map_model("my-custom-local-model-xyz");
        assert!(matches!(unknown.provider, MappedProvider::Unknown));
        assert_eq!(unknown.original, "my-custom-local-model-xyz");
        // Never panics on garbage.
        let _ = map_model("");
        let _ = map_model("\"quoted\"");
    }

    // -- secret detection ----------------------------------------------------

    #[test]
    fn secret_key_heuristics() {
        for key in [
            "apiKey",
            "api_key",
            "API_KEY",
            "api-key",
            "authToken",
            "access_token",
            "password",
            "clientSecret",
            "bearer",
            "token",
            "my_token",
            "token-xyz",
        ] {
            assert!(looks_like_secret_key(key), "key={}", key);
        }
        for key in [
            "model",
            "provider",
            "name",
            "command",
            "baseUrl",
            "temperature",
        ] {
            assert!(!looks_like_secret_key(key), "key={}", key);
        }
    }

    #[test]
    fn secret_value_heuristics() {
        // Known prefixes.
        assert!(looks_like_secret_value("sk-testvalue00000000000000000001"));
        assert!(looks_like_secret_value("ghp_testvalue000000000000000001"));
        assert!(looks_like_secret_value("AKIATESTVALUE0000000001"));
        // Long opaque string.
        assert!(looks_like_secret_value("abcdefghij1234567890ABCDEFGH12"));
        // Placeholders and short values are not secrets.
        assert!(!looks_like_secret_value("FAKE-KEY-FOR-TESTS-ONLY"));
        assert!(!looks_like_secret_value("your-api-key-here"));
        assert!(!looks_like_secret_value("short"));
        assert!(!looks_like_secret_value(""));
        assert!(!looks_like_secret_value("null"));
    }

    // -- Claude Code ----------------------------------------------------------

    #[test]
    fn claude_code_full_fixture() {
        let (_base, home, zhome) = home_and_zhome("claude");
        write(
            &home.join(".claude.json"),
            r#"{"mcpServers": {"gh": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"]}}}"#,
        );
        write(
            &home.join(".claude/settings.json"),
            r#"{"model": "claude-opus-4-6", "apiKey": "FAKE-KEY-FOR-TESTS-ONLY"}"#,
        );
        write(
            &home.join(".claude/skills/pdf/SKILL.md"),
            "---\nname: pdf\n---\n# PDF skill\n",
        );
        write(
            &home.join(".claude/commands/review.md"),
            "# Review command\n",
        );
        write(&home.join(".claude/CLAUDE.md"), "# Global memory\n");

        let plan = plan(&home, &zhome).unwrap();
        let kinds: Vec<ImportKind> = plan.items.iter().map(|i| i.kind).collect();
        assert!(
            kinds.contains(&ImportKind::McpServer),
            "mcp: {:?}",
            plan.items
        );
        assert!(kinds.contains(&ImportKind::Model));
        assert!(kinds.contains(&ImportKind::Skill));
        assert!(kinds.contains(&ImportKind::Command));
        assert!(kinds.contains(&ImportKind::Rule));

        // The fake apiKey is reported, never imported.
        assert_eq!(plan.secrets.len(), 1, "{:?}", plan.secrets);
        assert!(plan.secrets[0].name.contains("apiKey"));
        assert!(plan.secrets[0]
            .hint
            .contains("paste it into ZenCode Settings"));

        // Model mapped to Anthropic.
        let model = plan
            .items
            .iter()
            .find(|i| i.kind == ImportKind::Model)
            .unwrap();
        let mapping = model.model_mapping.as_ref().unwrap();
        assert!(matches!(mapping.provider, MappedProvider::Anthropic { .. }));
    }

    // -- Codex ------------------------------------------------------------------

    #[test]
    fn codex_toml_fixture() {
        let (_base, home, zhome) = home_and_zhome("codex");
        write(
            &home.join(".codex/config.toml"),
            "# comment\nmodel = \"gpt-5\"\nmodel_provider = \"openai\"\n\n[model_providers.openai]\nbase_url = \"https://api.openai.com/v1\"\n",
        );
        write(&home.join(".codex/auth.json"), r#"{"OPENAI_API_KEY": "x"}"#);

        let plan = plan(&home, &zhome).unwrap();
        let model = plan
            .items
            .iter()
            .find(|i| i.kind == ImportKind::Model)
            .expect("model item");
        let mapping = model.model_mapping.as_ref().unwrap();
        assert_eq!(mapping.suggested_name, "openai/gpt-5");
        // auth.json is secrets-only.
        assert!(plan.secrets.iter().any(|s| s.source.ends_with("auth.json")));
    }

    #[test]
    fn codex_custom_provider_kept_for_manual_mapping() {
        let (_base, home, zhome) = home_and_zhome("codex-custom");
        write(
            &home.join(".codex/config.toml"),
            "model = \"my-model\"\nmodel_provider = \"corp\"\n\n[model_providers.corp]\nbase_url = \"https://llm.corp.example/v1\"\n",
        );
        let plan = plan(&home, &zhome).unwrap();
        let model = plan
            .items
            .iter()
            .find(|i| i.kind == ImportKind::Model)
            .unwrap();
        assert!(matches!(
            model.model_mapping.as_ref().unwrap().provider,
            MappedProvider::Unknown
        ));
    }

    // -- Continue ---------------------------------------------------------------

    #[test]
    fn continue_yaml_fixture_with_trailing_section() {
        let (_base, home, zhome) = home_and_zhome("continue-yaml");
        write(
            &home.join(".continue/config.yaml"),
            "models:\n  - name: Work GPT\n    provider: openai\n    model: gpt-4o\n    apiKey: FAKE-KEY-FOR-TESTS-ONLY\n  - name: Local\n    provider: ollama\n    model: llama3.1\n# trailing top-level key exercises the flush-on-section-exit path\nallowAnonymousTelemetry: false\n",
        );
        let plan = plan(&home, &zhome).unwrap();
        let models: Vec<_> = plan
            .items
            .iter()
            .filter(|i| i.kind == ImportKind::Model)
            .collect();
        // Both models found, including the last one before the trailing key.
        assert_eq!(models.len(), 2, "{:?}", plan.items);
        assert!(plan.secrets.iter().any(|s| s.name == "Work GPT.apiKey"));
        let ollama = models.iter().find(|m| m.label.contains("Local")).unwrap();
        assert!(matches!(
            ollama.model_mapping.as_ref().unwrap().provider,
            MappedProvider::Ollama
        ));
    }

    #[test]
    fn continue_json_fixture() {
        let (_base, home, zhome) = home_and_zhome("continue-json");
        write(
            &home.join(".continue/config.json"),
            r#"{"models": [{"name": "Claude", "provider": "anthropic", "model": "claude-sonnet-4-6"}]}"#,
        );
        let plan = plan(&home, &zhome).unwrap();
        let model = plan
            .items
            .iter()
            .find(|i| i.kind == ImportKind::Model)
            .unwrap();
        assert!(matches!(
            model.model_mapping.as_ref().unwrap().provider,
            MappedProvider::Anthropic { .. }
        ));
    }

    // -- Aider / OpenCode / Cursor / Cline ---------------------------------------

    #[test]
    fn aider_yaml_fixture() {
        let (_base, home, zhome) = home_and_zhome("aider");
        write(
            &home.join(".aider.conf.yml"),
            "model: anthropic/claude-opus-4-6\nweak-model: gpt-4o-mini\napi-key: FAKE-KEY-FOR-TESTS-ONLY\n",
        );
        let plan = plan(&home, &zhome).unwrap();
        assert_eq!(
            plan.items
                .iter()
                .filter(|i| i.kind == ImportKind::Model)
                .count(),
            2
        );
        assert_eq!(plan.secrets.len(), 1);
    }

    #[test]
    fn opencode_json_fixture() {
        let (_base, home, zhome) = home_and_zhome("opencode");
        write(
            &home.join(".config/opencode/opencode.json"),
            r#"{
                "model": "anthropic/claude-opus-4-6",
                "mcp": {"fetch": {"type": "local", "command": ["uvx", "mcp-server-fetch"]}},
                "provider": {"anthropic": {"options": {"apiKey": "FAKE-KEY-FOR-TESTS-ONLY"}}}
            }"#,
        );
        let plan = plan(&home, &zhome).unwrap();
        assert!(plan.items.iter().any(|i| i.kind == ImportKind::Model));
        assert!(plan.items.iter().any(|i| i.kind == ImportKind::McpServer));
        assert!(plan
            .secrets
            .iter()
            .any(|s| s.name.contains("provider.anthropic")));
    }

    #[test]
    fn cursor_and_cline_mcp_fixtures() {
        let (_base, home, zhome) = home_and_zhome("cursor-cline");
        write(
            &home.join(".cursor/mcp.json"),
            r#"{"mcpServers": {"sqlite": {"command": "uvx", "args": ["mcp-server-sqlite"]}}}"#,
        );
        write(
            &home.join(
                ".config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
            ),
            r#"{"mcpServers": {"brave": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-brave-search"], "env": {"BRAVE_API_KEY": "FAKE-KEY-FOR-TESTS-ONLY"}}}}"#,
        );
        let plan = plan(&home, &zhome).unwrap();
        assert_eq!(
            plan.items
                .iter()
                .filter(|i| i.kind == ImportKind::McpServer)
                .count(),
            2
        );
        // The MCP env key is reported as a secret …
        assert!(plan
            .secrets
            .iter()
            .any(|s| s.name == "mcp env brave.BRAVE_API_KEY"));
    }

    // -- apply() -------------------------------------------------------------------

    #[test]
    fn apply_copies_skills_commands_rules_and_merges_mcp() {
        let (_base, home, zhome) = home_and_zhome("apply");
        write(
            &home.join(".claude/skills/pdf/SKILL.md"),
            "---\nname: pdf\n---\n",
        );
        write(&home.join(".claude/skills/pdf/helper.py"), "# helper\n");
        write(&home.join(".claude/commands/review.md"), "# review\n");
        write(&home.join(".claude/CLAUDE.md"), "# memory\n");
        write(
            &home.join(".claude.json"),
            r#"{"mcpServers": {"gh": {"command": "npx", "args": ["-y", "gh-mcp"], "env": {"GH_TOKEN": "FAKE-KEY-FOR-TESTS-ONLY"}}}}"#,
        );

        let plan = plan(&home, &zhome).unwrap();
        assert!(!plan.items.is_empty());
        let report = apply(&plan, &zhome).unwrap();

        // Skill dir copied recursively, marked UNTRUSTED.
        assert!(zhome.join("skills/pdf/SKILL.md").is_file());
        assert!(zhome.join("skills/pdf/helper.py").is_file());
        let skill = report
            .imported
            .iter()
            .find(|i| i.kind == ImportKind::Skill)
            .unwrap();
        assert!(skill.trust.contains("UNTRUSTED"));

        // Command + rule copied.
        assert!(zhome.join("commands/review.md").is_file());
        assert!(zhome.join("imported").read_dir().unwrap().count() == 1);

        // MCP merged, marked untrusted, secret env stripped.
        let mcp_text = fs::read_to_string(zhome.join("mcp.json")).unwrap();
        let mcp: Value = serde_json::from_str(&mcp_text).unwrap();
        assert!(mcp["mcpServers"]["claude-code-gh"]["command"] == "npx");
        assert_eq!(
            mcp["_zencode"]["trust"]["claude-code-gh"]["trust"],
            "untrusted"
        );
        assert!(
            mcp["mcpServers"]["claude-code-gh"]["env"]
                .as_object()
                .map(|o| !o.contains_key("GH_TOKEN"))
                .unwrap_or(true),
            "secret env must be stripped: {}",
            mcp_text
        );
        // … and the secret is in the report, never in the file.
        assert!(report.secrets.iter().any(|s| s.name.contains("GH_TOKEN")));
        assert!(!mcp_text.contains("FAKE-KEY-FOR-TESTS-ONLY"));
    }

    #[test]
    fn apply_never_overwrites_existing_files() {
        let (_base, home, zhome) = home_and_zhome("no-overwrite");
        write(
            &home.join(".claude/skills/pdf/SKILL.md"),
            "---\nname: pdf\n---\n",
        );
        // User already has their own pdf skill.
        write(&zhome.join("skills/pdf/SKILL.md"), "mine\n");

        let plan = plan(&home, &zhome).unwrap();
        // Pre-existing destination → deselected in the plan …
        let item = plan
            .items
            .iter()
            .find(|i| i.kind == ImportKind::Skill)
            .unwrap();
        assert!(!item.selected);

        // … and even a hand-selected one is refused at apply time.
        let mut forced = plan.clone();
        for i in forced.items.iter_mut() {
            i.selected = true;
        }
        let report = apply(&forced, &zhome).unwrap();
        assert_eq!(
            fs::read_to_string(zhome.join("skills/pdf/SKILL.md")).unwrap(),
            "mine\n"
        );
        assert!(report.imported.iter().all(|i| i.kind != ImportKind::Skill));
        assert!(report.skipped.iter().any(|s| s.contains("already exists")));
    }

    #[test]
    fn apply_merges_with_existing_mcp_json() {
        let (_base, home, zhome) = home_and_zhome("mcp-merge");
        write(
            &zhome.join("mcp.json"),
            r#"{"mcpServers": {"mine": {"command": "mine"}}}"#,
        );
        write(
            &home.join(".cursor/mcp.json"),
            r#"{"mcpServers": {"sqlite": {"command": "uvx"}}}"#,
        );
        let plan = plan(&home, &zhome).unwrap();
        // MCP items stay selected even though mcp.json exists (merge-safe).
        assert!(plan
            .items
            .iter()
            .filter(|i| i.kind == ImportKind::McpServer)
            .all(|i| i.selected));
        let report = apply(&plan, &zhome).unwrap();
        let mcp: Value =
            serde_json::from_str(&fs::read_to_string(zhome.join("mcp.json")).unwrap()).unwrap();
        // Existing server untouched, new one added.
        assert!(mcp["mcpServers"]["mine"]["command"] == "mine");
        assert!(mcp["mcpServers"]["cursor-sqlite"]["command"] == "uvx");
        assert!(report
            .imported
            .iter()
            .any(|i| i.label.contains("cursor-sqlite")));
    }

    #[test]
    fn apply_writes_model_suggestions_not_activations() {
        let (_base, home, zhome) = home_and_zhome("models");
        write(
            &home.join(".aider.conf.yml"),
            "model: deepseek/deepseek-chat\n",
        );
        let plan = plan(&home, &zhome).unwrap();
        let report = apply(&plan, &zhome).unwrap();
        let text = fs::read_to_string(zhome.join("imported-providers.json")).unwrap();
        assert!(text.contains("deepseek/deepseek-chat"));
        assert!(text.contains("never imported"));
        assert!(report.imported.iter().any(|i| i.id == "imported-providers"));
    }

    #[test]
    fn key_refusal_invariant_end_to_end() {
        // The strongest guarantee: no secret-looking string from any
        // competitor config may end up anywhere under zhome.
        let (_base, home, zhome) = home_and_zhome("refusal");
        write(
            &home.join(".continue/config.yaml"),
            "models:\n  - name: Paid\n    provider: openai\n    model: gpt-4o\n    apiKey: FAKE-KEY-FOR-TESTS-ONLY\n",
        );
        write(
            &home.join(".config/opencode/opencode.json"),
            r#"{"mcp": {"s": {"command": "x", "env": {"MY_TOKEN": "FAKE-KEY-FOR-TESTS-ONLY"}}}}"#,
        );
        let plan = plan(&home, &zhome).unwrap();
        assert!(!plan.secrets.is_empty());
        let report = apply(&plan, &zhome).unwrap();
        assert!(!report.secrets.is_empty());

        // Walk everything written and assert the fake key appears nowhere.
        fn walk(dir: &Path, hits: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    walk(&p, hits);
                } else if let Ok(text) = fs::read_to_string(&p) {
                    if text.contains("FAKE-KEY-FOR-TESTS-ONLY") {
                        hits.push(p);
                    }
                }
            }
        }
        let mut hits = Vec::new();
        walk(&zhome, &mut hits);
        assert!(hits.is_empty(), "secret leaked into: {:?}", hits);
    }
}
