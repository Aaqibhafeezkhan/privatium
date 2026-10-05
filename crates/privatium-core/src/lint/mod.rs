// This file is part of Privatium
// crates/privatium-core/src/lint/mod.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: `privatium lint` (spec/cli.md §5): the rule table with its stable IDs, severities and spec
//          citations; a finding and its JSON shape (§5.2); the walk over an app folder that
//          hands each file to the rule module that reads it — the manifest, the schema through
//          SQLite, Lua through a full_moon AST, templates through the compiler's own front
//          end, a Tier 2 web/ through a lexer — and the mechanical fixer of §5.3. A module of
//          the core, not a crate, so CI and the binary run identical rules.
// Notes: See README file for documentation and full license information.
//
// Copyright © 2026 Gabriel Mongefranco
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program. If not, see <https://www.gnu.org/licenses/>.

pub mod css;
pub mod html;
mod lua;
mod manifest;
pub mod spec_ref;
mod sql;
mod template;
mod web;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::app::manifest::{MANIFEST_FILE, Manifest};
use crate::config::Mode;
use crate::store::Schema;

/// How bad a finding is. Ordered, so `--severity warn` is "warn and above".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Reported for review; never a defect on its own.
    Info,
    /// Probably wrong.
    Warn,
    /// Wrong.
    Error,
}

impl Severity {
    /// As `--severity` and the JSON spell it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The rule classes of `spec/cli.md §5.1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// `PV1xx`.
    Contract,
    /// `PV2xx`.
    Security,
    /// `PV3xx`.
    Correctness,
    /// `PV4xx`.
    Accessibility,
    /// `PV5xx`.
    Portability,
}

impl Class {
    /// The heading `§5.1` gives the class.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contract => "Contract",
            Self::Security => "Security",
            Self::Correctness => "Correctness",
            Self::Accessibility => "Accessibility",
            Self::Portability => "Portability",
        }
    }
}

macro_rules! rule_ids {
    ($($id:ident),* $(,)?) => {
        /// A rule's stable ID (`spec/cli.md §5.1`: removing or renumbering one is a
        /// breaking change to the skills).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[allow(clippy::upper_case_acronyms)]
        pub enum RuleId {
            $(
                #[allow(missing_docs)]
                $id,
            )*
        }

        impl RuleId {
            /// Every rule, in `§5.1`'s order.
            pub const ALL: &'static [RuleId] = &[$(RuleId::$id),*];

            /// `PV301`.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(RuleId::$id => stringify!($id),)*
                }
            }

            /// The ID spelled `PV301`, if it is one.
            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                match text {
                    $(stringify!($id) => Some(RuleId::$id),)*
                    _ => None,
                }
            }
        }
    };
}

rule_ids! {
    PV101, PV102, PV103, PV104, PV105, PV106, PV107, PV108, PV109, PV110,
    PV111,
    PV201, PV202, PV203, PV204, PV205, PV206, PV207, PV208,
    PV301, PV302, PV303, PV304, PV305, PV306, PV307, PV308,
    PV401, PV402, PV403, PV404, PV405, PV406, PV407, PV408,
    PV501, PV502, PV503, PV504, PV505, PV506,
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One rule of `spec/cli.md §5.1`: what it says, how bad a finding is, what the linter
/// reads to judge it, and the document it enforces — the `spec` every finding carries
/// (`§5.2`: a rule that cannot cite one does not belong here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// The ID.
    pub id: RuleId,
    /// The class.
    pub class: Class,
    /// The severity of every finding of this rule.
    pub severity: Severity,
    /// The rule as `§5.1` states it.
    pub title: &'static str,
    /// The files the linter reads for it.
    pub reads: &'static str,
    /// The section that establishes the requirement: `spec/<file>.md §N.N`, or a document
    /// under `docs/` for the two icon rules.
    pub spec: &'static str,
    /// For `PV4xx`, the WCAG 2.2 success criterion behind it.
    pub criterion: Option<&'static str>,
}

/// The rule table. Every rule cites the spec section it enforces, which
/// `cargo xtask lint-spec-refs` resolves; a rule that cannot is not a rule.
pub static RULES: &[Rule] = &[
    Rule {
        id: RuleId::PV101,
        class: Class::Contract,
        severity: Severity::Error,
        title: "app.toml parses and carries slug, title, version, api, tier",
        reads: "app.toml",
        spec: "spec/app-contract.md §3",
        criterion: None,
    },
    Rule {
        id: RuleId::PV102,
        class: Class::Contract,
        severity: Severity::Error,
        title: "Slug matches ^[a-z][a-z0-9-]{1,30}$ and is not reserved",
        reads: "app.toml",
        spec: "spec/app-contract.md §3.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV103,
        class: Class::Contract,
        severity: Severity::Error,
        title: "api does not exceed the framework's supported version",
        reads: "app.toml",
        spec: "spec/protocol.md §12",
        criterion: None,
    },
    Rule {
        id: RuleId::PV104,
        class: Class::Contract,
        severity: Severity::Error,
        title: "Slug directory name matches app.slug",
        reads: "app.toml, the folder name",
        spec: "spec/app-contract.md §3.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV105,
        class: Class::Contract,
        severity: Severity::Error,
        title: "Tier-required files present and every Lua file and template parses",
        reads: "app.lua, web/index.html, lib/, views/",
        spec: "spec/app-contract.md §8",
        criterion: None,
    },
    Rule {
        id: RuleId::PV106,
        class: Class::Contract,
        severity: Severity::Error,
        title: "Every table in schema.sql has id VARCHAR PRIMARY KEY",
        reads: "schema.sql, through the engine's catalog",
        spec: "spec/app-contract.md §4.5",
        criterion: None,
    },
    Rule {
        id: RuleId::PV107,
        class: Class::Contract,
        severity: Severity::Error,
        title: "schema.sql contains only CREATE TABLE, CREATE VIEW, CREATE INDEX and comments",
        reads: "schema.sql, one statement at a time under the engine's authorizer",
        spec: "spec/app-contract.md §4.5",
        criterion: None,
    },
    Rule {
        id: RuleId::PV108,
        class: Class::Contract,
        severity: Severity::Error,
        title: "No UNIQUE constraint or index beyond id's primary key",
        reads: "schema.sql, through the engine's catalog",
        spec: "spec/app-contract.md §4.5",
        criterion: None,
    },
    Rule {
        id: RuleId::PV109,
        class: Class::Contract,
        severity: Severity::Error,
        title: "A document under the standard chrome has </head>, <body>, </body> and a main region with id=\"main\"",
        reads: "web/**/*.html, the views a layout() owns",
        spec: "spec/app-contract.md §5",
        criterion: None,
    },
    Rule {
        id: RuleId::PV110,
        class: Class::Contract,
        severity: Severity::Error,
        title: "Every [ui] reference resolves: scripts and styles exist under static/ and reach a framed view, menu items have a label and a mount-relative path, navigation = \"swap\" is for a Tier 1 app",
        reads: "app.toml, static/",
        spec: "spec/app-contract.md §3",
        criterion: None,
    },
    Rule {
        id: RuleId::PV111,
        class: Class::Contract,
        severity: Severity::Error,
        title: "A view of an app with navigation = \"swap\" carries no <script> or <link rel=\"stylesheet\">; they belong in ui.scripts and ui.styles",
        reads: "views/*.lsp, when app.toml sets [ui] navigation = \"swap\"",
        spec: "spec/lua-api.md §4.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV201,
        class: Class::Security,
        severity: Severity::Error,
        title: "No string-concatenated SQL — parameters must be bound",
        reads: "Lua and templates (AST), Tier 2 JavaScript",
        spec: "spec/lua-api.md §3.2",
        criterion: None,
    },
    Rule {
        id: RuleId::PV202,
        class: Class::Security,
        severity: Severity::Warn,
        title: "Every <?raw ?> use is reported for review",
        reads: "views/*.lsp",
        spec: "spec/lua-api.md §4",
        criterion: None,
    },
    Rule {
        id: RuleId::PV203,
        class: Class::Security,
        severity: Severity::Error,
        title: "No banned Lua global — io, os.execute, os.getenv, debug, load, dofile, package.loadlib, and the rest of the sandbox's closed list",
        reads: "Lua and templates (AST)",
        spec: "spec/lua-api.md §5",
        criterion: None,
    },
    Rule {
        id: RuleId::PV204,
        class: Class::Security,
        severity: Severity::Error,
        title: "Every non-GET form contains csrf()",
        reads: "views/*.lsp",
        spec: "spec/lua-api.md §4.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV205,
        class: Class::Security,
        severity: Severity::Warn,
        title: "Declared [permissions] beyond the defaults carry a justifying comment",
        reads: "app.toml",
        spec: "spec/app-contract.md §5.4",
        criterion: None,
    },
    Rule {
        id: RuleId::PV206,
        class: Class::Security,
        severity: Severity::Error,
        title: "No innerHTML with non-literal data in Tier 2 JavaScript",
        reads: "web/**/*.js",
        spec: "spec/app-contract.md §5.4",
        criterion: None,
    },
    Rule {
        id: RuleId::PV207,
        class: Class::Security,
        severity: Severity::Error,
        title: "No external origin referenced without a matching permissions.remote entry",
        reads: "templates, web/, static/",
        spec: "spec/app-contract.md §5.4",
        criterion: None,
    },
    Rule {
        id: RuleId::PV208,
        class: Class::Security,
        severity: Severity::Error,
        title: "No apparent secret in schema.sql, app.toml, or sample data",
        reads: "schema.sql, app.toml, sample/seed.jsonl",
        spec: "spec/protocol.md §3",
        criterion: None,
    },
    Rule {
        id: RuleId::PV301,
        class: Class::Correctness,
        severity: Severity::Error,
        title: "No literal /a/<slug>/ path — use url() or pv.url() (breaks solo mode)",
        reads: "Lua, templates, web/ (strings and attributes)",
        spec: "spec/app-contract.md §2.2",
        criterion: None,
    },
    Rule {
        id: RuleId::PV302,
        class: Class::Correctness,
        severity: Severity::Error,
        title: "No tonumber() or JavaScript Number() applied to a DECIMAL or BIGINT column",
        reads: "Lua and templates (AST) and web/ JavaScript, against schema.sql",
        spec: "spec/data-dictionary.md §2.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV303,
        class: Class::Correctness,
        severity: Severity::Error,
        title: "No INSERT, UPDATE, or DELETE in app SQL — writes are appends",
        reads: "the SQL literals of Lua, templates and JavaScript",
        spec: "spec/app-contract.md §7",
        criterion: None,
    },
    Rule {
        id: RuleId::PV304,
        class: Class::Correctness,
        severity: Severity::Error,
        title: "Client code does not set seq, lam, ts, dev, or app on an event",
        reads: "web/ JavaScript",
        spec: "spec/data-api.md §2",
        criterion: None,
    },
    Rule {
        id: RuleId::PV305,
        class: Class::Correctness,
        severity: Severity::Warn,
        title: "No outbox dedupe table, transaction ID, or acknowledgement protocol",
        reads: "schema.sql, Lua, JavaScript (names)",
        spec: "spec/protocol.md §10.6",
        criterion: None,
    },
    Rule {
        id: RuleId::PV306,
        class: Class::Correctness,
        severity: Severity::Warn,
        title: "Multi-event writes that must land together use pv.batch",
        reads: "Lua (AST), JavaScript",
        spec: "spec/lua-api.md §3.3",
        criterion: None,
    },
    Rule {
        id: RuleId::PV307,
        class: Class::Correctness,
        severity: Severity::Warn,
        title: "No global assigned in a handler expecting persistence, and no load-time table mutated from one",
        reads: "Lua (AST)",
        spec: "spec/lua-api.md §5",
        criterion: None,
    },
    Rule {
        id: RuleId::PV308,
        class: Class::Correctness,
        severity: Severity::Error,
        title: "No SUM() over a DECIMAL column and no + or - on a DATE column in app SQL",
        reads: "the SQL literals of Lua, templates and JavaScript, and CREATE VIEW in schema.sql",
        spec: "spec/data-dictionary.md §2",
        criterion: None,
    },
    Rule {
        id: RuleId::PV401,
        class: Class::Accessibility,
        severity: Severity::Error,
        title: "No icon-only control without a label argument or aria-label",
        reads: "templates, web/ HTML",
        spec: "docs/icons.md",
        criterion: Some("1.1.1 Non-text Content, 4.1.2 Name, Role, Value"),
    },
    Rule {
        id: RuleId::PV402,
        class: Class::Accessibility,
        severity: Severity::Error,
        title: "Every form input has an associated <label for>",
        reads: "templates, web/ HTML",
        spec: "spec/cli.md §5.1",
        criterion: Some("1.3.1 Info and Relationships, 3.3.2 Labels or Instructions"),
    },
    Rule {
        id: RuleId::PV403,
        class: Class::Accessibility,
        severity: Severity::Warn,
        title: "Radio and checkbox groups are wrapped in fieldset/legend",
        reads: "templates, web/ HTML",
        spec: "spec/cli.md §5.1",
        criterion: Some("1.3.1 Info and Relationships"),
    },
    Rule {
        id: RuleId::PV404,
        class: Class::Accessibility,
        severity: Severity::Warn,
        title: "Heading levels do not skip; exactly one <h1> per rendered page",
        reads: "templates as pages (a view with its partials, or a layout's document), web/ HTML",
        spec: "spec/cli.md §5.1",
        criterion: Some("1.3.1 Info and Relationships, 2.4.6 Headings and Labels"),
    },
    Rule {
        id: RuleId::PV405,
        class: Class::Accessibility,
        severity: Severity::Warn,
        title: "No status conveyed by colour alone",
        reads: "templates, web/ HTML",
        spec: "spec/cli.md §5.1",
        criterion: Some("1.4.1 Use of Color"),
    },
    Rule {
        id: RuleId::PV406,
        class: Class::Accessibility,
        severity: Severity::Warn,
        title: "Declared colour tokens meet 4.5:1 body / 3:1 large and UI",
        reads: "static/*.css, web/**/*.css",
        spec: "spec/cli.md §5.1",
        criterion: Some("1.4.3 Contrast (Minimum), 1.4.11 Non-text Contrast"),
    },
    Rule {
        id: RuleId::PV407,
        class: Class::Accessibility,
        severity: Severity::Warn,
        title: "Tabular data uses <table> with <th scope>, not a grid of divs",
        reads: "templates, web/ HTML",
        spec: "spec/cli.md §5.1",
        criterion: Some("1.3.1 Info and Relationships"),
    },
    Rule {
        id: RuleId::PV408,
        class: Class::Accessibility,
        severity: Severity::Warn,
        title: "No second way back — under the standard chrome the app draws no link of its own to the launcher or the settings pages",
        reads: "templates, web/ HTML and JavaScript",
        spec: "spec/app-contract.md §5",
        criterion: Some("3.2.3 Consistent Navigation"),
    },
    Rule {
        id: RuleId::PV501,
        class: Class::Portability,
        severity: Severity::Error,
        title: "Slug ≤ 15 characters when nav.advertise = true (DNS-SD label limit)",
        reads: "app.toml",
        spec: "spec/protocol.md §6.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV502,
        class: Class::Portability,
        severity: Severity::Error,
        title: "permissions.cross_origin_isolated only in solo mode",
        reads: "app.toml, the node's config.toml",
        spec: "spec/app-contract.md §5.4",
        criterion: None,
    },
    Rule {
        id: RuleId::PV503,
        class: Class::Portability,
        severity: Severity::Warn,
        title: "Icon names exist in the vendored Bootstrap Icons set",
        reads: "app.toml, Lua and templates (icon() calls)",
        spec: "docs/icons.md",
        criterion: None,
    },
    Rule {
        id: RuleId::PV504,
        class: Class::Portability,
        severity: Severity::Error,
        title: "No CDN reference — libraries are vendored under web/vendor/",
        reads: "templates, web/, static/",
        spec: "spec/app-contract.md §5.1",
        criterion: None,
    },
    Rule {
        id: RuleId::PV505,
        class: Class::Portability,
        severity: Severity::Error,
        title: "No absolute filesystem path, and nothing written beside the binary",
        reads: "Lua, templates, web/ (strings)",
        spec: "spec/protocol.md §3",
        criterion: None,
    },
    Rule {
        id: RuleId::PV506,
        class: Class::Portability,
        severity: Severity::Warn,
        title: "No app route matching a framework prefix — shadowed in solo mode",
        reads: "Lua routes (AST), web/ top-level entries",
        spec: "spec/protocol.md §9.1",
        criterion: None,
    },
];

/// The rule behind an ID.
#[must_use]
pub fn rule(id: RuleId) -> &'static Rule {
    RULES.iter().find(|rule| rule.id == id).unwrap_or(&RULES[0])
}

/// A mechanical correction (`spec/cli.md §5.3`): replace `start..end` of `file` with
/// `replacement`. Offsets are bytes into the file as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The file, as a path that can be opened.
    pub file: PathBuf,
    /// Start byte.
    pub start: usize,
    /// End byte, exclusive.
    pub end: usize,
    /// What goes in its place.
    pub replacement: String,
}

/// One finding: the seven fields of `spec/cli.md §5.2`, plus the edit `--fix` would make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The rule.
    pub id: RuleId,
    /// The rule's severity.
    pub severity: Severity,
    /// The file, as the caller named the app plus the path within it.
    pub file: String,
    /// 1-based line; `0` when the finding is about the file as a whole.
    pub line: u32,
    /// What is wrong.
    pub message: String,
    /// What to do about it, when the rule can say.
    pub fix: Option<String>,
    /// The section that establishes the requirement.
    pub spec: &'static str,
    /// The mechanical correction, when there is one.
    pub edit: Option<Edit>,
}

impl Finding {
    /// One JSON object, as `§5.2` shows it.
    #[must_use]
    pub fn json(&self) -> String {
        serde_json::json!({
            "id": self.id.as_str(),
            "severity": self.severity.as_str(),
            "file": self.file,
            "line": self.line,
            "message": self.message,
            "fix": self.fix,
            "spec": self.spec,
        })
        .to_string()
    }

    /// One line for a terminal.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = format!(
            "{}:{}: {} {}: {}",
            self.file, self.line, self.id, self.severity, self.message
        );
        if let Some(fix) = &self.fix {
            out.push_str(" — fix: ");
            out.push_str(fix);
        }
        out.push_str(&format!(" ({})", self.spec));
        out
    }
}

/// What the linter knows about the node it runs for: the mode decides `PV502` and
/// `PV506` (`spec/cli.md §5`: "plus the node configuration").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Options {
    /// `[node] mode`.
    pub mode: Mode,
    /// `[node] app` — the app mounted at `/` in solo mode.
    pub solo_app: Option<String>,
}

impl Options {
    /// From a node's configuration.
    #[must_use]
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            mode: config.node.mode,
            solo_app: config.node.app.clone(),
        }
    }
}

/// What one run found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Every finding, sorted by file, line and rule.
    pub findings: Vec<Finding>,
    /// The apps linted, as named.
    pub apps: Vec<String>,
}

impl Report {
    /// The findings at `severity` or above.
    pub fn at_or_above(&self, severity: Severity) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(move |f| f.severity >= severity)
    }
}

/// How far `discover` looks below a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// The folders the loader would mount: one level, `_`-prefixed names skipped.
    Installed,
    /// Any app folder up to three levels down — the lint corpus's `pass/<rule>/<slug>/`.
    Any,
}

/// The app folders at or under `path`: `path` itself when it holds `app.toml`, else its
/// descendants that do. Sorted.
#[must_use]
pub fn discover(path: &Path, depth: Depth) -> Vec<PathBuf> {
    if path.join(MANIFEST_FILE).is_file() {
        return vec![path.to_path_buf()];
    }
    let mut found = Vec::new();
    fn walk(dir: &Path, remaining: u32, installed: bool, into: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        children.sort();
        for child in children {
            let name = child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') || (installed && name.starts_with('_')) {
                continue;
            }
            if child.join(MANIFEST_FILE).is_file() {
                into.push(child);
            } else if remaining > 1 {
                walk(&child, remaining - 1, installed, into);
            }
        }
    }
    match depth {
        Depth::Installed => walk(path, 1, true, &mut found),
        Depth::Any => walk(path, 3, false, &mut found),
    }
    found
}

/// The state one app's rules share.
pub(crate) struct Ctx<'a> {
    /// The app folder.
    pub dir: &'a Path,
    /// How findings name the folder.
    pub display: &'a str,
    /// The node's mode.
    pub options: &'a Options,
    /// The folder name.
    pub folder: String,
    /// The manifest, when it parsed.
    pub manifest: Option<Manifest>,
    /// The schema, when there is one and it parsed.
    pub schema: Option<Schema>,
    /// Whether this app is the one mounted at `/`.
    pub solo: bool,
    /// Accumulated findings.
    pub findings: Vec<Finding>,
}

impl Ctx<'_> {
    /// The slug: the manifest's, or the folder name while there is no manifest.
    pub fn slug(&self) -> String {
        self.manifest
            .as_ref()
            .map_or_else(|| self.folder.clone(), |m| m.app.slug.clone())
    }

    /// `apps/hello/app.lua`.
    pub fn file(&self, rel: &str) -> String {
        format!("{}/{}", self.display.trim_end_matches(['/', '\\']), rel)
    }

    /// Record a finding; the caller may attach a fix and an edit to what is returned.
    pub fn push(
        &mut self,
        id: RuleId,
        rel: &str,
        line: u32,
        message: impl Into<String>,
    ) -> &mut Finding {
        let rule = rule(id);
        self.findings.push(Finding {
            id,
            severity: rule.severity,
            file: self.file(rel),
            line,
            message: message.into(),
            fix: None,
            spec: rule.spec,
            edit: None,
        });
        self.findings
            .last_mut()
            .unwrap_or_else(|| unreachable!("a finding was just pushed"))
    }

    /// `rel` read as text, or `None` with a finding when it exists and cannot be read.
    pub fn read(&mut self, rel: &str) -> Option<String> {
        let path = self.dir.join(rel);
        match fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                self.push(RuleId::PV105, rel, 0, format!("cannot be read: {error}"));
                None
            }
        }
    }

    /// The path the fixer opens.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }
}

/// Lint one app folder. `display` is how findings name it — `apps/hello`.
#[must_use]
pub fn lint_app(dir: &Path, display: &str, options: &Options) -> Vec<Finding> {
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut ctx = Ctx {
        dir,
        display,
        options,
        folder,
        manifest: None,
        schema: None,
        solo: false,
        findings: Vec::new(),
    };
    manifest::check(&mut ctx);
    ctx.solo = ctx.options.mode == Mode::Solo
        && ctx.options.solo_app.as_deref() == Some(ctx.slug().as_str());
    sql::check_schema(&mut ctx);
    manifest::check_seed(&mut ctx);
    let facts = lua::check_app(&mut ctx);
    template::check(&mut ctx, &facts);
    web::check(&mut ctx, &facts);
    let mut findings = ctx.findings;
    findings.sort_by(|a, b| {
        (a.file.as_str(), a.line, a.id, a.message.as_str()).cmp(&(
            b.file.as_str(),
            b.line,
            b.id,
            b.message.as_str(),
        ))
    });
    findings.dedup();
    findings
}

/// Lint every app at or under each path (`Depth::Any`). A path that is a file inside an
/// app lints that app and keeps the findings in that file; a path with no app under it
/// is one `PV101` finding.
#[must_use]
pub fn lint_paths(paths: &[PathBuf], options: &Options) -> Report {
    let mut report = Report::default();
    for given in paths {
        let display = given.to_string_lossy().replace('\\', "/");
        let display = display.trim_end_matches('/').to_owned();
        if given.is_file() {
            let Some((app, rel)) = enclosing_app(given) else {
                report.findings.push(Finding {
                    id: RuleId::PV101,
                    severity: Severity::Error,
                    file: display.clone(),
                    line: 0,
                    message: "is not inside an app folder — no app.toml above it".into(),
                    fix: Some("point the linter at the app folder".into()),
                    spec: rule(RuleId::PV101).spec,
                    edit: None,
                });
                continue;
            };
            let app_display = display
                .strip_suffix(&format!("/{rel}"))
                .unwrap_or(&display)
                .to_owned();
            let wanted = format!("{app_display}/{rel}");
            report.apps.push(app_display.clone());
            report.findings.extend(
                lint_app(&app, &app_display, options)
                    .into_iter()
                    .filter(|f| f.file == wanted),
            );
            continue;
        }
        let apps = discover(given, Depth::Any);
        if apps.is_empty() {
            report.findings.push(Finding {
                id: RuleId::PV101,
                severity: Severity::Error,
                file: format!("{display}/{MANIFEST_FILE}"),
                line: 0,
                message: "no app.toml here or in any folder beneath — an app is a folder \
                          containing one"
                    .into(),
                fix: None,
                spec: rule(RuleId::PV101).spec,
                edit: None,
            });
            continue;
        }
        for app in apps {
            let app_display = if app == *given {
                display.clone()
            } else {
                let rel = app
                    .strip_prefix(given)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                format!("{display}/{rel}")
            };
            report.apps.push(app_display.clone());
            report
                .findings
                .extend(lint_app(&app, &app_display, options));
        }
    }
    report
}

/// The app folder a file belongs to, and the file's path within it.
fn enclosing_app(file: &Path) -> Option<(PathBuf, String)> {
    let mut dir = file.parent()?;
    let mut rel: Vec<String> = vec![file.file_name()?.to_string_lossy().into_owned()];
    loop {
        if dir.join(MANIFEST_FILE).is_file() {
            rel.reverse();
            return Some((dir.to_path_buf(), rel.join("/")));
        }
        rel.push(dir.file_name()?.to_string_lossy().into_owned());
        dir = dir.parent()?;
    }
}

/// Apply every edit the findings carry (`spec/cli.md §5.3`), last-in-file first so
/// offsets hold. Returns the files rewritten. Overlapping edits in one file are refused
/// as a whole file, since the second would land on text the first changed.
pub fn apply(findings: &[Finding]) -> std::io::Result<Vec<PathBuf>> {
    let mut by_file: BTreeMap<PathBuf, Vec<&Edit>> = BTreeMap::new();
    for edit in findings.iter().filter_map(|f| f.edit.as_ref()) {
        by_file.entry(edit.file.clone()).or_default().push(edit);
    }
    let mut written = Vec::new();
    for (file, mut edits) in by_file {
        edits.sort_by(|a, b| b.start.cmp(&a.start));
        edits.dedup_by(|a, b| a.start == b.start && a.end == b.end);
        let overlapping = edits.windows(2).any(|pair| pair[1].end > pair[0].start);
        if overlapping {
            continue;
        }
        let mut text = fs::read_to_string(&file)?;
        for edit in edits {
            if edit.end > text.len()
                || !text.is_char_boundary(edit.start)
                || !text.is_char_boundary(edit.end)
            {
                continue;
            }
            text.replace_range(edit.start..edit.end, &edit.replacement);
        }
        fs::write(&file, text)?;
        written.push(file);
    }
    Ok(written)
}

/// `PV109` over one document under the standard chrome (`spec/app-contract.md §5`): the
/// three anchors the chrome is inserted at, and a `<main id="main">` for the skip link
/// to land in. `text` is the document as served, or a layout's literal text; findings are
/// recorded against `rel`.
pub(crate) fn check_chrome_anchors(ctx: &mut Ctx<'_>, rel: &str, text: &str) {
    if let Err(anchor) = crate::http::apps::chrome_anchors(text) {
        ctx.push(
            RuleId::PV109,
            rel,
            1,
            format!("no {anchor} — the standard chrome is inserted before </head>, after <body> and before </body>, and without the tag the document is served bare"),
        )
        .fix = Some("write the tag, or set [ui] chrome = \"none\" to own the whole document".into());
    }
    let root = html::parse(text);
    if !root
        .find_all("main")
        .iter()
        .any(|main| main.attr("id") == Some("main"))
    {
        let line = root.find_all("main").first().map_or(1, |main| main.line);
        ctx.push(
            RuleId::PV109,
            rel,
            line,
            "no <main id=\"main\"> — the chrome's skip link targets #main",
        )
        .fix = Some("give the main region id=\"main\"".into());
    }
}

/// `PV408`: an `href` that is the launcher or a framework settings page. Under the
/// standard chrome the bar already carries both, and a second way back is one more
/// control to learn (`spec/app-contract.md §5`).
pub(crate) fn is_way_back_href(value: &str) -> bool {
    let value = value.trim();
    value == "/" || value == "/settings" || value.starts_with("/settings/")
}

/// `PV408`: a string in browser code that climbs out of the mount to the launcher, as
/// `pv.url('../../')` does from `/a/<slug>/`.
pub(crate) fn is_way_back_path(value: &str) -> bool {
    matches!(value.trim(), "../../" | "../..")
}

/// The 1-based line of byte `offset` in `text`.
pub(crate) fn line_of(text: &str, offset: usize) -> u32 {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count() as u32
        + 1
}

/// The origin of an absolute URL — `https://host[:port]` — if `text` is one. A
/// protocol-relative `//host/…` counts too, since a browser resolves it to an origin
/// that is not the node's.
#[must_use]
pub fn origin_of(text: &str) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = if let Some(rest) = text.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = text.strip_prefix("http://") {
        ("http", rest)
    } else if let Some(rest) = text.strip_prefix("//") {
        ("https", rest)
    } else {
        return None;
    };
    let host = rest
        .split(['/', '?', '#', '\'', '"', ' ', ')', '<', '>'])
        .next()
        .unwrap_or_default();
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b':')
        || !host.contains('.') && host != "localhost"
    {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

/// Whether `text` is a literal filesystem path an app must not carry (`PV505`): a POSIX
/// home or system prefix, a Windows drive, or `~`.
#[must_use]
pub fn is_absolute_fs_path(text: &str) -> bool {
    let text = text.trim();
    let posix = [
        "/home/", "/Users/", "/var/", "/etc/", "/tmp/", "/opt/", "/usr/", "/root/", "/mnt/",
        "/srv/",
    ];
    if posix.iter().any(|p| text.starts_with(p))
        || text.starts_with("~/")
        || text.starts_with("file://")
    {
        return true;
    }
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// The `/a/<slug>/…` literal at the start of `text`, as `(slug, path beneath the mount)`,
/// when `text` is exactly such a path (`PV301`).
#[must_use]
pub fn mount_path(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("/a/")?;
    let (slug, beneath) = rest.split_once('/')?;
    if !crate::app::manifest::is_valid_slug(slug) {
        return None;
    }
    if beneath.contains(char::is_whitespace) || beneath.contains('<') {
        return None;
    }
    Some((slug.to_owned(), format!("/{beneath}")))
}

/// The tables and columns of a schema by declared kind, for the rules that read a
/// column name: `PV302` (DECIMAL, BIGINT) and `PV308` (DECIMAL, DATE).
#[derive(Debug, Clone, Default)]
pub(crate) struct Columns {
    /// Column name → every declared type it has across tables, uppercased.
    pub by_name: BTreeMap<String, Vec<String>>,
}

impl Columns {
    pub fn of(schema: Option<&Schema>) -> Self {
        let mut by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some(schema) = schema {
            for table in &schema.tables {
                for column in &table.columns {
                    by_name
                        .entry(column.name.to_ascii_lowercase())
                        .or_default()
                        .push(column.ty.trim().to_ascii_uppercase());
                }
            }
        }
        Self { by_name }
    }

    fn has(&self, name: &str, test: impl Fn(&str) -> bool) -> bool {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .is_some_and(|types| types.iter().any(|t| test(t)))
    }

    /// A `DECIMAL`/`NUMERIC` column of that name exists.
    pub fn is_decimal(&self, name: &str) -> bool {
        self.has(name, |t| {
            t.starts_with("DECIMAL") || t.starts_with("NUMERIC")
        })
    }

    /// A `BIGINT`/`INTEGER` column of that name exists.
    pub fn is_integer(&self, name: &str) -> bool {
        self.has(name, |t| !t.starts_with("INTERVAL") && t.contains("INT"))
    }

    /// A `DATE`, `TIME` or `TIMESTAMPTZ` column of that name exists.
    pub fn is_date(&self, name: &str) -> bool {
        self.has(name, |t| {
            t == "DATE" || t == "TIMESTAMPTZ" || t == "TIME" || t == "TIMESTAMP"
        })
    }
}

// AGENTS.md, Style: unwrap() is permitted in tests. The crate-level deny reaches unit
// tests inside src/, so each one opts out where it is declared.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_has_one_row_and_a_spec_ref() {
        assert_eq!(RULES.len(), RuleId::ALL.len());
        for (rule, id) in RULES.iter().zip(RuleId::ALL) {
            assert_eq!(rule.id, *id, "the table is in §5.1's order");
            assert!(!rule.spec.is_empty());
            assert_eq!(RuleId::parse(id.as_str()), Some(*id));
        }
        assert_eq!(RuleId::parse("PV999"), None);
    }

    #[test]
    fn the_json_has_the_seven_fields() {
        let finding = Finding {
            id: RuleId::PV301,
            severity: Severity::Error,
            file: "apps/meds/app.lua".into(),
            line: 42,
            message: "Literal mount path".into(),
            fix: Some("Use url('/')".into()),
            spec: "spec/app-contract.md §2.2",
            edit: None,
        };
        let value: serde_json::Value = serde_json::from_str(&finding.json()).unwrap();
        for key in ["id", "severity", "file", "line", "message", "fix", "spec"] {
            assert!(value.get(key).is_some(), "{key}");
        }
        assert_eq!(value["line"], 42);
        assert!(
            finding
                .text()
                .starts_with("apps/meds/app.lua:42: PV301 error:")
        );
    }

    #[test]
    fn origins_paths_and_mounts_are_recognized() {
        assert_eq!(
            origin_of("https://cdn.example.com/x.js").as_deref(),
            Some("https://cdn.example.com")
        );
        assert_eq!(
            origin_of("//cdn.example.com/x.js").as_deref(),
            Some("https://cdn.example.com")
        );
        assert_eq!(
            origin_of("http://127.0.0.1:8420/a"),
            Some("http://127.0.0.1:8420".into())
        );
        assert_eq!(origin_of("/static/pv.js"), None);
        assert_eq!(origin_of("https://"), None);
        assert!(is_absolute_fs_path("C:\\Users\\x\\file.txt"));
        assert!(is_absolute_fs_path("/home/x/notes"));
        assert!(!is_absolute_fs_path("/static/x.css"));
        assert_eq!(
            mount_path("/a/meds/edit"),
            Some(("meds".into(), "/edit".into()))
        );
        assert_eq!(mount_path("/a/meds/"), Some(("meds".into(), "/".into())));
        assert_eq!(mount_path("/a/meds"), None);
        assert_eq!(mount_path("/api/x"), None);
    }

    #[test]
    fn apply_rewrites_from_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.lua");
        fs::write(&file, "aa bb cc").unwrap();
        let edit = |start, end, rep: &str| Finding {
            id: RuleId::PV301,
            severity: Severity::Error,
            file: "t.lua".into(),
            line: 1,
            message: String::new(),
            fix: None,
            spec: "",
            edit: Some(Edit {
                file: file.clone(),
                start,
                end,
                replacement: rep.into(),
            }),
        };
        let written = apply(&[edit(0, 2, "X"), edit(6, 8, "YYY")]).unwrap();
        assert_eq!(written, vec![file.clone()]);
        assert_eq!(fs::read_to_string(&file).unwrap(), "X bb YYY");
    }
}
