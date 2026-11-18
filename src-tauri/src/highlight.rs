//! TOML-backed syntax-highlight rule loading and safe test matching (M6b).
//!
//! Rule files are plain text stored in the portable application's highlight
//! directory. Regexes use Rust's linear-time regex engine and are size bounded.

use std::fs;
use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::paths;

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SAMPLE_BYTES: usize = 64 * 1024;
const MAX_RULES: usize = 128;
const MAX_PATTERN_BYTES: usize = 2 * 1024;
const MAX_MATCHES_PER_RULE: usize = 500;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum RuleScope { Prompt, Command, Output, Any }

impl Default for RuleScope {
    fn default() -> Self { Self::Any }
}

impl RuleScope {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Command => "command",
            Self::Output => "output",
            Self::Any => "any",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct HighlightRule {
    name: String,
    #[serde(default)]
    scope: RuleScope,
    pattern: String,
    #[serde(default)]
    fg: Option<String>,
    #[serde(default)]
    bg: Option<String>,
    #[serde(default)]
    bold: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RuleSet {
    name: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default, rename = "rule")]
    rules: Vec<HighlightRule>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleSetStatus {
    pub name: String,
    pub file: String,
    pub rule_count: usize,
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HighlightTestMatch {
    pub line: usize,
    pub rule_name: String,
    pub scope: String,
    pub start: usize,
    pub end: usize,
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HighlightTestResult {
    pub set_name: String,
    pub prompt_matches: bool,
    pub matches: Vec<HighlightTestMatch>,
}

fn compile(pattern: &str) -> Result<Regex, String> {
    if pattern.is_empty() || pattern.len() > MAX_PATTERN_BYTES {
        return Err("pattern must contain 1–2048 bytes".into());
    }
    RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .nest_limit(64)
        .build()
        .map_err(|error| format!("invalid regex: {error}"))
}

fn validate_color(value: &Option<String>) -> Result<(), String> {
    if let Some(color) = value {
        let valid = color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            return Err(format!("invalid color '{color}'; expected #RRGGBB"));
        }
    }
    Ok(())
}

fn parse_rule_set(path: &Path) -> Result<RuleSet, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err("rule file exceeds 256 KiB".into());
    }
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let set: RuleSet = toml::from_str(&source).map_err(|error| error.to_string())?;
    if set.name.trim().is_empty() {
        return Err("rule set name cannot be empty".into());
    }
    if set.rules.len() > MAX_RULES {
        return Err("rule set exceeds 128 rules".into());
    }
    if let Some(prompt) = &set.prompt { compile(prompt)?; }
    let mut names = std::collections::BTreeSet::new();
    for rule in &set.rules {
        if rule.name.trim().is_empty() { return Err("rule name cannot be empty".into()); }
        if !names.insert(rule.name.to_ascii_lowercase()) {
            return Err(format!("duplicate rule name '{}'", rule.name));
        }
        compile(&rule.pattern)?;
        validate_color(&rule.fg)?;
        validate_color(&rule.bg)?;
    }
    Ok(set)
}

fn rule_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml")) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn display_name(path: &Path) -> String {
    path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("invalid").to_string()
}

#[tauri::command]
pub fn highlight_list() -> Result<Vec<RuleSetStatus>, String> {
    let mut output = Vec::new();
    for path in rule_files(&paths::highlight_dir())? {
        let file = path.file_name().and_then(|name| name.to_str()).unwrap_or("").to_string();
        match parse_rule_set(&path) {
            Ok(set) => output.push(RuleSetStatus {
                name: set.name, file, rule_count: set.rules.len(), valid: true, error: None,
            }),
            Err(error) => output.push(RuleSetStatus {
                name: display_name(&path), file, rule_count: 0, valid: false, error: Some(error),
            }),
        }
    }
    Ok(output)
}

#[tauri::command]
pub fn highlight_test(set_name: String, sample: String) -> Result<HighlightTestResult, String> {
    if sample.len() > MAX_SAMPLE_BYTES { return Err("sample exceeds 64 KiB".into()); }
    for path in rule_files(&paths::highlight_dir())? {
        let set = match parse_rule_set(&path) { Ok(set) => set, Err(_) => continue };
        if set.name != set_name { continue; }
        let prompt_matches = set.prompt.as_deref().map(compile).transpose()?
            .is_some_and(|regex| regex.is_match(&sample));
        let lines: Vec<String> = sample.split('\n').map(|line| line.trim_end_matches('\r').to_string()).collect();
        let buffer_matches = match_rule_sets(std::slice::from_ref(&set), &lines)?;
        let matches = buffer_matches.into_iter().map(|found| HighlightTestMatch {
            line: found.line,
            rule_name: found.rule_name,
            scope: found.scope,
            start: found.start,
            end: found.end,
            fg: found.fg,
            bg: found.bg,
            bold: found.bold,
        }).collect();
        return Ok(HighlightTestResult { set_name, prompt_matches, matches });
    }
    Err("highlight_rule_set_not_found".into())
}


#[derive(Debug, Clone, Serialize)]
pub struct HighlightBufferMatch {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub rule_name: String,
    pub scope: String,
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
}

fn match_rule_sets(rule_sets: &[RuleSet], lines: &[String]) -> Result<Vec<HighlightBufferMatch>, String> {
    let mut output = Vec::new();
    for set in rule_sets {
        let prompt_regex = set.prompt.as_deref().map(compile).transpose()?;
        for (line_index, line) in lines.iter().enumerate() {
            let prompt_end = prompt_regex.as_ref()
                .and_then(|regex| regex.find(line))
                .map(|found| found.end());
            for rule in &set.rules {
                let regex = compile(&rule.pattern)?;
                let (text, offset) = match rule.scope {
                    RuleScope::Prompt => match prompt_end {
                        Some(end) => (&line[..end], 0),
                        None => continue,
                    },
                    RuleScope::Command => match prompt_end {
                        Some(end) => (&line[end..], end),
                        None => continue,
                    },
                    RuleScope::Output if prompt_end.is_some() => continue,
                    RuleScope::Output | RuleScope::Any => (line.as_str(), 0),
                };
                for found in regex.find_iter(text).take(MAX_MATCHES_PER_RULE) {
                    output.push(HighlightBufferMatch {
                        line: line_index,
                        start: line[..offset + found.start()].chars().count(),
                        end: line[..offset + found.end()].chars().count(),
                        rule_name: rule.name.clone(),
                        scope: rule.scope.as_str().into(),
                        fg: rule.fg.clone(),
                        bg: rule.bg.clone(),
                        bold: rule.bold,
                    });
                }
            }
        }
    }
    Ok(output)
}

/// Match already-parsed terminal lines using Rust's bounded regex engine.
/// The PTY byte stream is never modified; the frontend uses these ranges only
/// to register xterm.js cell decorations.
#[tauri::command]
pub fn highlight_match(rule_sets: Vec<String>, lines: Vec<String>) -> Result<Vec<HighlightBufferMatch>, String> {
    if rule_sets.len() > 32 || lines.len() > 100 {
        return Err("highlight_request_too_large".into());
    }
    let total_bytes: usize = lines.iter().map(String::len).sum();
    if total_bytes > MAX_SAMPLE_BYTES || lines.iter().any(|line| line.len() > 4096) {
        return Err("highlight_request_too_large".into());
    }
    if rule_sets.is_empty() || lines.is_empty() {
        return Ok(Vec::new());
    }

    let wanted: std::collections::BTreeSet<&str> = rule_sets.iter().map(String::as_str).collect();
    let mut parsed_sets = Vec::new();
    for path in rule_files(&paths::highlight_dir())? {
        let set = match parse_rule_set(&path) {
            Ok(set) if wanted.contains(set.name.as_str()) => set,
            _ => continue,
        };
        parsed_sets.push(set);
    }
    match_rule_sets(&parsed_sets, &lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_rule_file_and_matches_sample() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linux.toml");
        fs::write(&path, r##"
name = "linux"
prompt = '^\w+@\w+'

[[rule]]
name = "ip"
scope = "any"
pattern = '\b(?:\d{1,3}\.){3}\d{1,3}\b'
fg = "#e3b341"
bold = true
"##).unwrap();
        let set = parse_rule_set(&path).unwrap();
        assert_eq!(set.name, "linux");
        let regex = compile(&set.rules[0].pattern).unwrap();
        assert_eq!(regex.find("host 10.0.0.1").unwrap().as_str(), "10.0.0.1");
    }

    #[test]
    fn rejects_invalid_regex() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-regex.toml");
        fs::write(&path, r##"
name = "bad"
[[rule]]
name = "broken"
pattern = "("
"##).unwrap();
        assert!(parse_rule_set(&path).unwrap_err().contains("invalid regex"));
    }

    #[test]
    fn rejects_invalid_colors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad-color.toml");
        fs::write(&path, r##"
name = "bad"
[[rule]]
name = "ip"
pattern = 'ip'
fg = "#12"
"##).unwrap();
        assert!(parse_rule_set(&path).unwrap_err().contains("invalid color"));
    }


    #[test]
    fn matches_multiple_buffer_lines_with_character_ranges() {
        let set = RuleSet {
            name: "test".into(),
            prompt: None,
            rules: vec![HighlightRule {
                name: "ip".into(),
                scope: RuleScope::Any,
                pattern: r"10\.0\.0\.1".into(),
                fg: Some("#e3b341".into()),
                bg: None,
                bold: true,
            }],
        };
        let lines = vec!["host 10.0.0.1".into(), "no address".into()];
        let matches = match_rule_sets(&[set], &lines).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!((matches[0].line, matches[0].start, matches[0].end), (0, 5, 13));
        assert!(matches[0].bold);
    }


    #[test]
    fn scoped_rules_distinguish_prompt_command_and_output() {
        let set = RuleSet {
            name: "scopes".into(),
            prompt: Some(r"^user@host:\$ ".into()),
            rules: vec![
                HighlightRule { name: "prompt".into(), scope: RuleScope::Prompt, pattern: r"^user@host:".into(), fg: None, bg: None, bold: false },
                HighlightRule { name: "command".into(), scope: RuleScope::Command, pattern: r"^sudo".into(), fg: None, bg: None, bold: false },
                HighlightRule { name: "output".into(), scope: RuleScope::Output, pattern: r"ERROR".into(), fg: None, bg: None, bold: false },
            ],
        };
        let lines = vec!["user@host:$ sudo reboot".into(), "ERROR: service stopped".into()];
        let matches = match_rule_sets(&[set], &lines).unwrap();
        assert!(matches.iter().any(|item| item.rule_name == "prompt" && item.line == 0));
        assert!(matches.iter().any(|item| item.rule_name == "command" && item.line == 0));
        assert!(matches.iter().any(|item| item.rule_name == "output" && item.line == 1));
        assert!(!matches.iter().any(|item| item.rule_name == "output" && item.line == 0));
    }

    #[test]
    fn rejects_oversized_samples() {
        assert!(highlight_test("missing".into(), "x".repeat(MAX_SAMPLE_BYTES + 1)).is_err());
    }
}
