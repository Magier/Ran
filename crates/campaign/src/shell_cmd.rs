//! Shell command analysis using a POSIX shell AST (via yash-syntax).
//!
//! The key type is [`ShellCmd`], which parses a shell command string and
//! exposes its structure: pipeline stages, simple command invocations,
//! environment assignments, and arguments.  Binary path grounding uses the
//! AST to find invocation sites precisely, rather than doing naive string
//! substitution.

use std::collections::HashMap;
use std::ops::Range;

use ran_domain::BinaryPresence;
use yash_syntax::syntax::{AndOrList, Command, Item, List, MaybeLiteral, Pipeline, SimpleCommand};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed shell command ready for structural analysis and binary grounding.
///
/// ```text
/// export TOKEN=abc; echo '{}' | kubectl apply -f - && kubectl wait pod/foo
///
/// List
/// ├── Item (;)
/// │   └── AndOrList
/// │       └── Pipeline [stage]
/// │           └── SimpleCommand  assigns=[TOKEN=abc]  words=[export]
/// └── Item
///     └── AndOrList
///         ├── Pipeline (first)
///         │   ├── SimpleCommand  words=[echo, '{}']
///         │   └── SimpleCommand  words=[kubectl, apply, -f, -]
///         └── Pipeline (&&)
///             └── SimpleCommand  words=[kubectl, wait, pod/foo]
/// ```
pub struct ShellCmd {
    source: String,
    entries: Vec<Entry>,
}

/// One simple command extracted from the shell AST.
#[derive(Debug, Clone)]
pub struct SimpleCmd {
    /// Environment variable assignments that precede the command name.
    /// E.g. `FOO=bar cmd` → `[("FOO", "bar")]`.
    pub assigns: Vec<(String, String)>,
    /// The command name (argv\[0\]), when it is a bare literal word.
    /// `None` for pure-assignment commands or when argv\[0\] contains
    /// expansions (e.g. `$cmd` or `"kubectl"`).
    pub name: Option<String>,
    /// Arguments after the command name, rendered as shell text.
    pub args: Vec<String>,
}

/// Internal record — owns position info needed for grounding.
#[derive(Debug)]
struct Entry {
    cmd: SimpleCmd,
    /// Char-index range of the command name in `source`, for replacement.
    name_range: Option<Range<usize>>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

impl ShellCmd {
    /// Parse `source` into a shell command AST.
    ///
    /// Returns `Err` with a human-readable message when `source` is not valid
    /// POSIX shell syntax.  Empty or whitespace-only strings produce an empty
    /// `ShellCmd` (no error).
    pub fn parse(source: &str) -> Result<Self, String> {
        let list: List = source
            .parse()
            .map_err(|e: yash_syntax::parser::Error| e.to_string())?;
        let entries = collect_entries(&list);
        Ok(Self {
            source: source.to_string(),
            entries,
        })
    }

    // ---------------------------------------------------------------------------
    // Analysis
    // ---------------------------------------------------------------------------

    /// Iterate over all simple commands found in the command string, in source order.
    pub fn commands(&self) -> impl Iterator<Item = &SimpleCmd> {
        self.entries.iter().map(|e| &e.cmd)
    }

    /// All bare tool names that appear as a command name (argv\[0\]) in any
    /// pipeline stage.  Excludes names that already contain `/`.
    pub fn tool_names(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter_map(|e| e.cmd.name.as_deref())
            .filter(|n| !n.contains('/'))
            .collect()
    }

    // ---------------------------------------------------------------------------
    // Grounding
    // ---------------------------------------------------------------------------

    /// Ground all bare command names against a binary map.
    ///
    /// For every simple command whose name is a plain literal (no path separators)
    /// that maps to a `BinaryPresence::Present(path)` with a different path,
    /// replaces the occurrence in the source string and returns the result.
    ///
    /// Falls back to returning the original source unchanged if no replacements
    /// are needed.
    pub fn ground(&self, binaries: &HashMap<String, BinaryPresence>) -> String {
        let mut replacements: Vec<(Range<usize>, String)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let name = e.cmd.name.as_deref()?;
                let range = e.name_range.clone()?;
                if name.contains('/') {
                    return None;
                }
                match binaries.get(name) {
                    Some(BinaryPresence::Present(path))
                        if !path.is_empty() && path.as_str() != name =>
                    {
                        Some((range, path.clone()))
                    }
                    _ => None,
                }
            })
            .collect();

        if replacements.is_empty() {
            return self.source.clone();
        }

        // Apply from end to start so earlier char indices stay valid.
        replacements.sort_by_key(|b| std::cmp::Reverse(b.0.start));

        // The AST location ranges are char-indexed; Rust strings are byte-indexed.
        let char_indices: Vec<usize> = self
            .source
            .char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(self.source.len()))
            .collect();
        let char_to_byte = |ci: usize| char_indices.get(ci).copied().unwrap_or(self.source.len());

        let mut result = self.source.clone();
        for (char_range, replacement) in replacements {
            let byte_start = char_to_byte(char_range.start);
            let byte_end = char_to_byte(char_range.end);
            result.replace_range(byte_start..byte_end, &replacement);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// AST walking
// ---------------------------------------------------------------------------

fn collect_entries(list: &List) -> Vec<Entry> {
    let mut out = Vec::new();
    for item in &list.0 {
        collect_from_item(item, &mut out);
    }
    out
}

fn collect_from_item(item: &Item, out: &mut Vec<Entry>) {
    collect_from_and_or(&item.and_or, out);
}

fn collect_from_and_or(and_or: &AndOrList, out: &mut Vec<Entry>) {
    collect_from_pipeline(&and_or.first, out);
    for (_, pipeline) in &and_or.rest {
        collect_from_pipeline(pipeline, out);
    }
}

fn collect_from_pipeline(pipeline: &Pipeline, out: &mut Vec<Entry>) {
    for cmd in &pipeline.commands {
        match cmd.as_ref() {
            Command::Simple(sc) => out.push(entry_from_simple(sc)),
            Command::Compound(_) | Command::Function(_) => {
                // TTP procedures don't use compound commands or function defs;
                // skip rather than recursing into bodies that may not be relevant.
            }
        }
    }
}

fn entry_from_simple(sc: &SimpleCommand) -> Entry {
    let assigns: Vec<(String, String)> = sc
        .assigns
        .iter()
        .map(|a| (a.name.clone(), a.value.to_string()))
        .collect();

    let (name, name_range) = match sc.words.first() {
        Some((word, _)) => {
            let literal = word.to_string_if_literal();
            let range = word.location.range.clone();
            (literal, Some(range))
        }
        None => (None, None),
    };

    let args: Vec<String> = sc
        .words
        .iter()
        .skip(1)
        .map(|(w, _)| w.to_string())
        .collect();

    Entry {
        cmd: SimpleCmd {
            assigns,
            name,
            args,
        },
        name_range,
    }
}

// ---------------------------------------------------------------------------
// Free-function façade used by execution.rs
// ---------------------------------------------------------------------------

/// Ground all command-name occurrences in `cmd` against a binary map.
///
/// Parses `cmd` with a full shell AST and replaces every bare tool invocation
/// whose name is `Present(path)` in `binaries`.  Falls back to the source
/// string unchanged when parsing fails or no replacements are needed.
pub fn ground_binaries(cmd: &str, binaries: &HashMap<String, BinaryPresence>) -> String {
    match ShellCmd::parse(cmd) {
        Ok(shell) => shell.ground(binaries),
        Err(_) => cmd.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn present(path: &str) -> BinaryPresence {
        BinaryPresence::Present(path.to_string())
    }

    #[test]
    fn parses_simple_command() {
        let sc = ShellCmd::parse("kubectl get pods").unwrap();
        let cmds: Vec<_> = sc.commands().collect();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name.as_deref(), Some("kubectl"));
        assert_eq!(cmds[0].args, ["get", "pods"]);
    }

    #[test]
    fn parses_pipeline() {
        let sc = ShellCmd::parse("echo '{}' | kubectl apply -f -").unwrap();
        let cmds: Vec<_> = sc.commands().collect();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name.as_deref(), Some("echo"));
        assert_eq!(cmds[1].name.as_deref(), Some("kubectl"));
    }

    #[test]
    fn parses_and_or_chain() {
        let sc = ShellCmd::parse("kubectl apply -f - && kubectl wait pod/foo").unwrap();
        let cmds: Vec<_> = sc.commands().collect();
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().all(|c| c.name.as_deref() == Some("kubectl")));
    }

    #[test]
    fn parses_assignment_before_command() {
        let sc = ShellCmd::parse("FOO=bar env").unwrap();
        let cmds: Vec<_> = sc.commands().collect();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name.as_deref(), Some("env"));
        assert_eq!(cmds[0].assigns, [("FOO".to_string(), "bar".to_string())]);
    }

    #[test]
    fn parses_sequential_commands() {
        let sc = ShellCmd::parse("export TOKEN=abc; echo hello").unwrap();
        let cmds: Vec<_> = sc.commands().collect();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name.as_deref(), Some("export"));
        assert_eq!(cmds[1].name.as_deref(), Some("echo"));
    }

    #[test]
    fn grounds_first_word_command() {
        let sc = ShellCmd::parse("kubectl get pods").unwrap();
        let mut map = HashMap::new();
        map.insert("kubectl".to_string(), present("/tmp/kubectl"));
        let result = sc.ground(&map);
        assert_eq!(result, "/tmp/kubectl get pods");
    }

    #[test]
    fn grounds_inline_tool_in_pipeline() {
        let cmd = "export TOKEN=abc; echo '{}' | kubectl apply -f - && kubectl wait pod/foo";
        let sc = ShellCmd::parse(cmd).unwrap();
        let mut map = HashMap::new();
        map.insert("kubectl".to_string(), present("/tmp/kubectl"));
        let result = sc.ground(&map);
        assert!(result.contains("| /tmp/kubectl apply"));
        assert!(result.contains("&& /tmp/kubectl wait"));
        assert!(!result.contains("| kubectl"));
        assert!(!result.contains("&& kubectl"));
    }

    #[test]
    fn ground_binaries_facade_works() {
        let mut map = HashMap::new();
        map.insert("kubectl".to_string(), present("/opt/bin/kubectl"));
        let result = ground_binaries("echo x | kubectl apply -f -", &map);
        assert!(result.contains("/opt/bin/kubectl"));
    }

    #[test]
    fn tool_names_returns_bare_names() {
        let sc = ShellCmd::parse("echo '{}' | kubectl apply -f - && kubectl wait pod/foo").unwrap();
        let names = sc.tool_names();
        assert_eq!(names, ["echo", "kubectl", "kubectl"]);
    }

    #[test]
    fn no_replacement_when_already_absolute() {
        let sc = ShellCmd::parse("/tmp/kubectl get pods").unwrap();
        let mut map = HashMap::new();
        map.insert("kubectl".to_string(), present("/tmp/kubectl"));
        let result = sc.ground(&map);
        assert_eq!(result, "/tmp/kubectl get pods");
    }
}
