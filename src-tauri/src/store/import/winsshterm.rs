//! WinSSHTerm `connections.xml` importer (M4, step 3c).
//!
//! WinSSHTerm stores every entry as a `<Node>` element and distinguishes them
//! by a `Type` attribute: `Type="Connection"` is a saved session and
//! `Type="Container"` is a folder. Containers nest, so a connection's folder
//! path is the list of enclosing container names, from the outermost down.
//!
//! The parser also stays schema-tolerant for other exports: records may be
//! attribute- or child-element-based, and folders may be encoded as nested
//! container tags, as a `Folder`/`Path` attribute on the record, or as a
//! `<Folder>` child element. Passwords are never imported; WinSSHTerm encrypts
//! them with its own key, so a warning is shown instead.

use std::path::PathBuf;

use crate::vault::Auth;

use super::{default_user, home_dir, ImportCandidate, ImportPreview, Skipped};

const HOST_KEYS: &[&str] = &[
    "hostname", "host", "hostaddress", "server", "serveraddress", "address", "ip",
    "ipaddress", "url", "destination",
];
const NAME_KEYS: &[&str] = &[
    "name", "displayname", "caption", "title", "label", "connectionname", "description",
];
const PORT_KEYS: &[&str] = &["port", "portnumber", "sshport"];
const USER_KEYS: &[&str] = &["username", "user", "login", "loginname", "user_name"];
const KEY_KEYS: &[&str] = &["identityfile", "privatekey", "keyfile", "privatekeyfile", "key", "identity"];
const PASSWORD_KEYS: &[&str] = &["password", "pass", "passphrase"];

/// Attribute names carrying the node type in WinSSHTerm exports.
const TYPE_KEYS: &[&str] = &["type", "nodetype", "kind"];
const CONTAINER_TYPES: &[&str] = &["container", "folder", "group", "category", "directory", "dir"];
const RECORD_TYPES: &[&str] = &["connection", "session", "server", "host", "ssh"];

/// Tag-name fallback for exports that do not use a `Type` attribute.
const FOLDER_TAG_KEYS: &[&str] = &[
    "folder", "group", "category", "container", "connections", "sessions", "root", "tree", "node",
];
/// Child elements / attributes that carry a folder path on the record itself.
const FOLDER_FIELD_KEYS: &[&str] = &[
    "folder", "folderpath", "group", "grouppath", "category", "categorypath",
    "path", "location", "parent", "parentpath",
];
const PATH_ATTR_KEYS: &[&str] = &[
    "folder", "folderpath", "group", "grouppath", "category", "categorypath",
    "path", "pathname", "location", "parent", "parentpath", "tree",
];

fn default_file() -> Result<PathBuf, &'static str> {
    Ok(home_dir()?.join("Documents").join("SSHTerm").join("connections.xml"))
}

pub fn parse(path: Option<&str>) -> Result<ImportPreview, &'static str> {
    let file = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => default_file()?,
    };
    let text = std::fs::read_to_string(&file).map_err(|_| "import_read_failed")?;
    Ok(parse_text(&text))
}

fn pick(attrs: &[(String, String)], keys: &[&str]) -> Option<String> {
    attrs
        .iter()
        .find(|(k, v)| keys.contains(&k.as_str()) && !v.trim().is_empty())
        .map(|(_, v)| v.trim().to_string())
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Splits a path written as `A/B`, `A\B` or `A > B` into folder names.
fn split_path(raw: &str) -> Vec<String> {
    raw.split(|c| c == '/' || c == '\\' || c == '>')
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .map(str::to_string)
        .collect()
}

/// Merges ancestor folder names with an explicit path attribute. An absolute
/// explicit path replaces the ancestors; a relative one is appended, skipping
/// a leading segment that repeats the innermost ancestor (common in exports
/// that repeat the parent folder name on every connection).
fn compose_path(mut ancestors: Vec<String>, explicit: Option<&str>) -> Vec<String> {
    let Some(raw) = explicit else { return ancestors };
    let parts = split_path(raw);
    if parts.is_empty() {
        return ancestors;
    }
    if raw.trim_start().starts_with(|c| c == '/' || c == '\\') {
        return parts;
    }
    for part in parts {
        if ancestors.last().is_some_and(|last| last.eq_ignore_ascii_case(&part)) {
            continue;
        }
        ancestors.push(part);
    }
    ancestors
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Folder,
    Record,
    Other,
}

/// An explicit `Type` attribute wins over the tag name: WinSSHTerm uses the
/// same `<Node>` tag for folders and connections.
fn classify(tag: &str, attrs: &[(String, String)]) -> NodeKind {
    let lower = tag.to_ascii_lowercase();
    if let Some(value) = pick(attrs, TYPE_KEYS) {
        let value = value.to_ascii_lowercase();
        if CONTAINER_TYPES.contains(&value.as_str()) {
            return NodeKind::Folder;
        }
        if RECORD_TYPES.contains(&value.as_str()) {
            return NodeKind::Record;
        }
    }
    if ["connection", "session", "server", "host"]
        .iter()
        .any(|k| lower.ends_with(k))
        || pick(attrs, HOST_KEYS).is_some()
    {
        return NodeKind::Record;
    }
    if FOLDER_TAG_KEYS.iter().any(|k| lower.ends_with(k)) {
        return NodeKind::Folder;
    }
    NodeKind::Other
}

fn field_kind(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if HOST_KEYS.contains(&lower.as_str()) {
        Some("host")
    } else if NAME_KEYS.contains(&lower.as_str()) {
        Some("name")
    } else if PORT_KEYS.contains(&lower.as_str()) {
        Some("port")
    } else if USER_KEYS.contains(&lower.as_str()) {
        Some("user")
    } else if KEY_KEYS.contains(&lower.as_str()) {
        Some("key")
    } else if PASSWORD_KEYS.contains(&lower.as_str()) {
        Some("password")
    } else if FOLDER_FIELD_KEYS.contains(&lower.as_str()) {
        Some("folder")
    } else {
        None
    }
}

fn parse_tag(body: &str) -> (String, Vec<(String, String)>) {
    let end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
    let name = body[..end].trim().to_ascii_lowercase();
    let rest = &body[end..];
    let bytes = rest.as_bytes();
    let mut attrs = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let key = rest[start..i].to_ascii_lowercase();
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'=') {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
            let value_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            attrs.push((key, decode_entities(&rest[value_start..i])));
            continue;
        }
        let quote = bytes[i];
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        attrs.push((key, decode_entities(&rest[value_start..i])));
        if i < bytes.len() {
            i += 1;
        }
    }
    (name, attrs)
}

enum Event {
    Start(String, Vec<(String, String)>),
    End,
    Text(String),
}

fn events(xml: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let bytes = xml.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            let text = decode_entities(xml[start..i].trim());
            if !text.is_empty() {
                out.push(Event::Text(text));
            }
            continue;
        }
        if xml[i..].starts_with("<!--") {
            match xml[i + 4..].find("-->") {
                Some(end) => i += 4 + end + 3,
                None => break,
            }
            continue;
        }
        if xml[i..].starts_with("<?") || xml[i..].starts_with("<!") {
            match xml[i..].find('>') {
                Some(end) => i += end + 1,
                None => break,
            }
            continue;
        }
        let Some(end) = xml[i..].find('>') else { break };
        let tag = xml[i + 1..i + end].trim();
        i += end + 1;
        if tag.is_empty() {
            continue;
        }
        if tag.starts_with('/') {
            out.push(Event::End);
            continue;
        }
        let self_closing = tag.ends_with('/');
        let (name, attrs) = parse_tag(tag.trim_end_matches('/').trim());
        if name.is_empty() {
            continue;
        }
        out.push(Event::Start(name.clone(), attrs));
        if self_closing {
            out.push(Event::End);
        }
    }
    out
}

/// One open XML element. `name` is filled from the `Name` attribute or, for
/// child-element-based exports, from a nested `<Name>` element.
struct Frame {
    kind: NodeKind,
    name: Option<String>,
}

/// Full folder path for the record at the top of `stack`. Index 0 is the
/// document root wrapper, which is not a user-visible folder.
fn ancestor_folders(stack: &[Frame]) -> Vec<String> {
    stack
        .iter()
        .take(stack.len().saturating_sub(1))
        .enumerate()
        .filter(|(index, frame)| *index > 0 && frame.kind == NodeKind::Folder)
        .filter_map(|(_, frame)| frame.name.clone())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// Routes a field value either into the open record or, when no record is
/// open, into the name of the enclosing container frame.
fn route_field(
    field: &str,
    value: &str,
    depth: usize,
    stack: &mut [Frame],
    draft: &mut Option<Draft>,
) {
    if let Some(d) = draft.as_mut() {
        d.set_text(field, value);
        return;
    }
    if field != "name" || depth < 2 {
        return;
    }
    let parent = &mut stack[depth - 2];
    if parent.kind == NodeKind::Folder && parent.name.is_none() {
        let value = value.trim().to_string();
        if !value.is_empty() {
            parent.name = Some(value);
        }
    }
}

struct Draft {
    depth: usize,
    folder_path: Vec<String>,
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    key_file: Option<String>,
    has_any_field: bool,
    warnings: Vec<String>,
}

impl Draft {
    fn from_attrs(depth: usize, folder_path: Vec<String>, attrs: &[(String, String)]) -> Self {
        let mut draft = Draft {
            depth,
            folder_path,
            name: None,
            host: None,
            port: None,
            user: None,
            key_file: None,
            has_any_field: false,
            warnings: Vec::new(),
        };
        draft.apply_attrs(attrs);
        draft
    }

    fn apply_attrs(&mut self, attrs: &[(String, String)]) {
        self.name = pick(attrs, NAME_KEYS).or(self.name.take());
        self.host = pick(attrs, HOST_KEYS).or(self.host.take());
        self.user = pick(attrs, USER_KEYS).or(self.user.take());
        self.key_file = pick(attrs, KEY_KEYS).or(self.key_file.take());
        if let Some(port) = pick(attrs, PORT_KEYS) {
            self.port = port.parse::<u16>().ok().filter(|p| *p > 0).or(self.port);
        }
        if pick(attrs, PASSWORD_KEYS).is_some() {
            self.warnings.push("winsshterm_password_not_imported".into());
        }
        self.has_any_field = self.name.is_some()
            || self.host.is_some()
            || self.user.is_some()
            || self.key_file.is_some()
            || self.port.is_some();
    }

    fn set_text(&mut self, kind: &str, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        self.has_any_field = true;
        match kind {
            "name" => self.name = Some(value.to_string()),
            "host" => self.host = Some(value.to_string()),
            "user" => self.user = Some(value.to_string()),
            "key" => self.key_file = Some(value.to_string()),
            "port" => self.port = value.parse::<u16>().ok().filter(|p| *p > 0).or(self.port),
            "folder" => self.folder_path.extend(split_path(value)),
            "password" => self.warnings.push("winsshterm_password_not_imported".into()),
            _ => {}
        }
    }

    fn finish(self) -> Result<Option<ImportCandidate>, Skipped> {
        if !self.has_any_field {
            return Ok(None);
        }
        let name = self.name.clone().unwrap_or_else(|| self.host.clone().unwrap_or_default());
        let label = if self.folder_path.is_empty() {
            name.clone()
        } else {
            format!("{} / {}", self.folder_path.join(" / "), name)
        };
        let Some(host) = self.host.filter(|h| !h.is_empty()) else {
            return Err(Skipped { label, reason: "missing_host".into() });
        };
        let auth = match self.key_file.filter(|p| !p.is_empty()) {
            Some(path) => Auth::Key { path, passphrase: None },
            None => Auth::Agent,
        };
        Ok(Some(ImportCandidate {
            name,
            host,
            port: self.port.unwrap_or(22),
            user: self.user.unwrap_or_else(default_user),
            auth,
            label,
            folder_path: self.folder_path,
            warnings: self.warnings,
        }))
    }
}

pub fn parse_text(xml: &str) -> ImportPreview {
    let mut out = ImportPreview::new("winsshterm");
    let mut stack: Vec<Frame> = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut pending: Option<(&'static str, usize)> = None;

    for event in events(xml) {
        match event {
            Event::Start(name, attrs) => {
                let kind = classify(&name, &attrs);
                stack.push(Frame { kind, name: pick(&attrs, NAME_KEYS) });
                let depth = stack.len();
                if kind == NodeKind::Record && draft.is_none() {
                    let explicit = pick(&attrs, PATH_ATTR_KEYS);
                    let folder_path = compose_path(ancestor_folders(&stack), explicit.as_deref());
                    draft = Some(Draft::from_attrs(depth, folder_path, &attrs));
                    pending = None;
                    continue;
                }
                if let Some(field) = field_kind(&name) {
                    if let Some(value) = pick(&attrs, &["value", "text", "content"]) {
                        route_field(field, &value, depth, &mut stack, &mut draft);
                    }
                    pending = Some((field, depth));
                }
            }
            Event::Text(text) => {
                if let Some((field, depth)) = pending {
                    route_field(field, &text, depth, &mut stack, &mut draft);
                }
            }
            Event::End => {
                let closing_depth = stack.len();
                if pending.is_some_and(|(_, depth)| depth == closing_depth) {
                    pending = None;
                }
                if draft.as_ref().is_some_and(|d| d.depth == closing_depth) {
                    match draft.take().unwrap().finish() {
                        Ok(Some(candidate)) => out.candidates.push(candidate),
                        Ok(None) => {}
                        Err(skipped) => out.skipped.push(skipped),
                    }
                    pending = None;
                }
                stack.pop();
            }
        }
    }
    if out.candidates.is_empty() && out.skipped.is_empty() {
        out.notices.push("winsshterm_no_records".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTR_XML: &str = r#"<?xml version="1.0"?>
<Connections>
  <Folder Name="Production">
    <Connection Name="pgw-01" HostName="10.0.0.5" Port="2222" UserName="admin" />
    <Connection Name="db-01" HostName="10.0.0.6" User="dba" IdentityFile="C:\keys\id" />
  </Folder>
  <Connection Name="no-host" User="someone" />
</Connections>"#;

    const CHILD_XML: &str = r#"<Sessions>
  <Group Name="Lab">
    <Session>
      <Name>lab-01</Name>
      <HostName>192.168.1.2</HostName>
      <Port>22</Port>
      <UserName>rodol</UserName>
    </Session>
  </Group>
</Sessions>"#;

    /// Shape of a real WinSSHTerm export: a single `<Node>` tag with a `Type`
    /// discriminator and recursively nested containers. All values here are
    /// synthetic; no real host, user or credential is used in tests.
    const NODE_XML: &str = r#"<WinSSHTerm>
  <Node Name="root-session" Type="Connection" Username="alice" Hostname="10.0.0.1" Port="22" PrivateKey="" />
  <Node Name="Production" Type="Container" Expanded="True" TabPrefix="">
    <Node Name="db-01" Type="Connection" Username="dba" Hostname="10.0.0.5" Port="2222" />
    <Node Name="Edge" Type="Container" Expanded="False">
      <Node Name="gw-01" Type="Connection" Username="admin" Hostname="10.0.0.9" />
    </Node>
  </Node>
</WinSSHTerm>"#;

    const PATH_XML: &str = r#"<Connections>
  <Connection Name="a" HostName="h1" Folder="Prod\Edge" />
  <Connection Name="b" HostName="h2"><Folder>Lab/Test</Folder></Connection>
</Connections>"#;

    const CHILD_FOLDER_XML: &str = r#"<Root>
  <Folder><Name>Lab</Name>
    <Session><Name>lab-01</Name><HostName>192.168.1.2</HostName></Session>
  </Folder>
</Root>"#;

    #[test]
    fn reads_attribute_based_records_with_folder_names() {
        let preview = parse_text(ATTR_XML);
        assert_eq!(preview.candidates.len(), 2);
        assert_eq!(preview.candidates[0].name, "pgw-01");
        assert_eq!(preview.candidates[0].host, "10.0.0.5");
        assert_eq!(preview.candidates[0].port, 2222);
        assert_eq!(preview.candidates[0].label, "Production / pgw-01");
        assert_eq!(preview.candidates[0].folder_path, vec!["Production"]);
        assert!(matches!(preview.candidates[1].auth, Auth::Key { .. }));
        assert!(preview.skipped.iter().any(|s| s.reason == "missing_host"));
    }

    #[test]
    fn reads_child_element_based_records() {
        let preview = parse_text(CHILD_XML);
        assert_eq!(preview.candidates.len(), 1);
        let c = &preview.candidates[0];
        assert_eq!(c.name, "lab-01");
        assert_eq!(c.host, "192.168.1.2");
        assert_eq!(c.user, "rodol");
        assert_eq!(c.label, "Lab / lab-01");
    }

    #[test]
    fn collects_the_full_ancestor_path_of_nested_node_containers() {
        let preview = parse_text(NODE_XML);
        assert_eq!(preview.candidates.len(), 3);
        assert_eq!(preview.candidates[0].folder_path, Vec::<String>::new());
        assert_eq!(preview.candidates[1].folder_path, vec!["Production"]);
        assert_eq!(preview.candidates[2].folder_path, vec!["Production", "Edge"]);
        assert_eq!(preview.candidates[2].label, "Production / Edge / gw-01");
        assert_eq!(preview.candidates[2].port, 22);
    }

    #[test]
    fn container_nodes_are_never_mistaken_for_connections() {
        let preview = parse_text(NODE_XML);
        assert!(preview.skipped.is_empty());
        assert!(preview.notices.is_empty());
        assert!(preview.candidates.iter().all(|c| c.host.starts_with("10.0.0.")));
    }

    #[test]
    fn explicit_folder_attributes_and_child_elements_become_paths() {
        let preview = parse_text(PATH_XML);
        assert_eq!(preview.candidates.len(), 2);
        assert_eq!(preview.candidates[0].folder_path, vec!["Prod", "Edge"]);
        assert_eq!(preview.candidates[1].folder_path, vec!["Lab", "Test"]);
    }

    #[test]
    fn folder_name_can_come_from_a_child_element() {
        let preview = parse_text(CHILD_FOLDER_XML);
        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].label, "Lab / lab-01");
        assert_eq!(preview.candidates[0].folder_path, vec!["Lab"]);
    }

    #[test]
    fn unrecognised_schema_is_reported() {
        let preview = parse_text("<Something><Else/></Something>");
        assert!(preview.candidates.is_empty());
        assert_eq!(preview.notices, vec!["winsshterm_no_records".to_string()]);
    }

    #[test]
    fn default_file_is_under_documents() {
        let path = default_file().unwrap();
        assert!(path.display().to_string().ends_with("connections.xml"));
    }
}
