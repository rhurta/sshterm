---
name: sshterm-highlight-rules
description: Create or improve SSHTerm TOML syntax-highlighting rule files from real terminal-output examples. Use when a user supplies terminal screenshots, copied shell output, prompts, commands, or asks for a new highlight profile.
---

# Create SSHTerm highlight rule files from examples

Create accurate, conservative SSHTerm syntax-highlighting profiles from terminal examples supplied by the user. The output is a TOML rule-set file compatible with SSHTerm's M6b highlighter.

## Workflow

1. **Inspect the source example.**
   - Use the exact terminal text or screenshot supplied by the user. If it is a screenshot, transcribe only text that is legible; do not invent hidden characters or prompt details.
   - Identify the shell or tool only when the example provides evidence. Do not assume Bash, Zsh, PowerShell, or a specific Linux distribution from appearance alone.
   - Separate prompt, typed command, command arguments, and program output.
   - Note whether the prompt is stable, variable, multiline, or absent. If the sample does not contain enough evidence to write a reliable prompt expression, ask for a copied prompt line rather than fabricating one.

2. **Inspect the repository schema and existing examples when working in the SSHTerm repository.**
   - Read `src-tauri/src/highlight.rs` and `examples/highlight/*.toml` before authoring. Treat the current implementation as authoritative if this skill and source code differ.
   - Check existing profiles to avoid duplicating a rule set name or rule purpose.
   - A rule file is loaded from the `highlight/` directory beside the portable `sshterm.exe`. Tracked examples live under `examples/highlight/`; they are not loaded from that directory automatically.

3. **Design a small, ordered rule set.**
   - Start with the narrowest useful rules: prompt, command name, common command options/parameters, addresses or paths, and clearly recognizable output keywords.
   - Use `scope = "prompt"` for prompt-only text, `scope = "command"` for the command portion after the detected prompt, `scope = "output"` for output lines, and `scope = "any"` only for tokens that should be highlighted anywhere.
   - Set the top-level `prompt` regex to match the prompt prefix, including its trailing delimiter (usually a space). It is used to divide the prompt from the command. Anchor it at the start of the line unless the observed prompt format requires otherwise.
   - The prompt expression should match the prompt only, not the command that follows it. Avoid hard-coding a particular username, hostname, home directory, or current path unless the user explicitly wants a profile for that one fixed environment.
   - Prefer explicit token boundaries and conservative patterns. Avoid rules that color ordinary words in unrelated output.
   - TOML rule order is retained. Give each rule a unique, descriptive `name`.

4. **Use only supported fields and syntax.**
   - Top level: `name`, optional `prompt`, and one or more `[[rule]]` tables.
   - Each rule: required `name` and `pattern`; optional `scope`, `fg`, `bg`, and `bold`.
   - Supported scopes are exactly `prompt`, `command`, `output`, and `any`. If omitted, scope defaults to `any`.
   - Colors must be six-digit hex strings such as `"#8fb4e8"` (format `#RRGGBB`). `bold` is a boolean.
   - Patterns use Rust's `regex` crate syntax, not PCRE, JavaScript, or Oniguruma. Do not use look-around, backreferences, conditionals, or other unsupported constructs. Use non-capturing groups `(?:...)` where grouping is needed.
   - TOML literal single-quoted strings are convenient for regexes because backslashes remain literal. Escape quotes correctly if using basic double-quoted TOML strings.
   - Keep within implementation limits: 256 KiB per file, 128 rules per set, 2,048 bytes per regex, and 64 KiB for tester samples. Prefer substantially smaller profiles.

5. **Validate against the supplied sample.**
   - Test the prompt regex against each representative prompt line and verify that it ends exactly at the prompt delimiter.
   - Walk through every proposed rule and identify the exact sample text it should match. Check that it does not unintentionally match the prompt, command, or output when its scope excludes them.
   - If the repository's Settings → Syntax highlighting rules and tester is available, load the profile and run the original sample plus at least one negative/control sample.
   - When code execution or the tester is unavailable, clearly state that the TOML was reviewed but not runtime-tested. Never claim a test passed without running it.
   - Correct invalid TOML, unsupported regex constructs, invalid colors, duplicate rule names, and overly broad matches before delivering the file.

6. **Deliver the result.**
   - Provide the complete TOML file, not fragments, and use a descriptive lowercase filename such as `linux-bash.toml` or `powershell.toml`.
   - Explain which prompt format and terminal conventions the profile supports, and mention important assumptions or known gaps.
   - For repository changes, place the file in `examples/highlight/` unless the user explicitly requests installation into the runtime `highlight/` directory. Update relevant documentation if the example is intended to be maintained in the repository.
   - Tell the user to copy the example into the `highlight/` folder beside `sshterm.exe`, then select it in the connection's Highlighting settings or a folder's inherited rules.
   - If the sample is ambiguous, provide a safe partial profile and list the missing evidence, or ask one focused question before generating a profile. Do not silently guess.

## Quality checklist

Before finalizing, confirm:

- [ ] The file parses as TOML.
- [ ] The rule-set `name` is unique among installed profiles.
- [ ] Every rule has a unique `name` and a valid Rust-regex pattern.
- [ ] Every scope is supported.
- [ ] Every color is `#RRGGBB`.
- [ ] Prompt detection matches the prompt prefix and not the command.
- [ ] Rules use the narrowest correct scope and avoid noisy false positives.
- [ ] The original sample and a negative/control sample have been tested, or the lack of runtime testing is disclosed.
- [ ] The complete file and installation instructions are included in the response.

## Minimal structure

```toml
name = "example-shell"
prompt = '^user@host:[^\\n]*[$#] '

[[rule]]
name = "prompt"
scope = "prompt"
pattern = '^user@host:'
fg = "#8fb4e8"
bold = true

[[rule]]
name = "command"
scope = "command"
pattern = '^\\s*[A-Za-z0-9_.-]+'
fg = "#e3b341"
bold = true

[[rule]]
name = "error-keyword"
scope = "output"
pattern = '\\b(?:ERROR|FAILED|FATAL)\\b'
fg = "#bf616a"
bold = true
```

The structure is illustrative only. Replace the fixed prompt with a pattern supported by the user's actual example; do not copy the sample prompt literally unless it matches.
