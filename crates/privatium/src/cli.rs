// This file is part of Privatium
// crates/privatium/src/cli.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-04
// Last Modified: 2026-10-15
// Summary: The argument grammar of spec/cli.md, by hand. The surface is eight commands and twenty flags
//          fixed by a normative document, and the help text is that document's synopsis lines
//          — so there is no derive layer to drift from it, no dependency to carry for it, and
//          `test_no_undocumented_flags` compares the two directly. A mistake here is a usage
//          error, exit 2 (§1); nothing here touches a node.
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

use std::ffi::OsString;
use std::path::PathBuf;

use privatium_core::app::manifest::Tier;

/// `spec/cli.md`, one synopsis per section, in the spec's order and spelling. Printed by
/// `--help` and after every usage error; the flag names in it are the CLI's whole surface.
pub const HELP: &str = "\
privatium [--data-dir <path>] [--config <file>] [--verbose] [--version] [<command> [args]]

  privatium [--port 8420] [--solo <slug>] [--no-discovery] [--open]
      run a node (spec/cli.md §2)
  privatium dev [--app <slug>] [--open]
      the development loop: a node, and the app to edit (§3)
  privatium new <slug> [--tier lua|web|rust] [--from <existing-app>] [--scaffold <table>]
  privatium new --examples
      scaffold an app under <data-dir>/apps/<slug>/, or write every example app (§4)
  privatium install <zip> [--yes]
      install an app from a zip file (§4.1)
  privatium lint [<path>...] [--format text|json] [--severity error|warn|info] [--fix]
      the linter (§5)
  privatium skill list
  privatium skill export [<name>...] [--out <dir>]
      the skills this build ships, for an assistant (§6)
  privatium snapshot [--app <slug>] [--verify]
  privatium restore --from <path> [--app <slug>] [--dry-run]
      snapshots and the three-tier restore (§7)
  privatium pair [--open] [--timeout 120] [--node] [--join <url>]
      pairing mode, for devices or for another space; --join joins the space at <url> (§8)
  privatium firewall [--apply]
      the firewall helper (§9)

  --data-dir  the node's data root; by default a privatium-data folder beside the
              program when you created one, else the platform data directory
  --config    config.toml; <data-dir>/config.toml by default
  --verbose   report what the node loaded and maintained, not only what failed
  --version   the build version and the protocol it implements

Exit codes: 0 success, 1 runtime error, 2 usage error, 3 lint findings present (§1).
";

/// The flags of `§1`, which every command shares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Global {
    pub data_dir: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub verbose: bool,
}

/// `lint --format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

/// `lint --severity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

/// One parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `--version`.
    Version,
    /// `--help` / `-h`.
    Help,
    /// Bare `privatium` (`§2`).
    Run {
        port: Option<u16>,
        solo: Option<String>,
        no_discovery: bool,
        open: bool,
    },
    /// `§3`.
    Dev { app: Option<String>, open: bool },
    /// `§4`.
    New {
        slug: String,
        tier: Option<Tier>,
        from: Option<String>,
        scaffold: Option<String>,
    },
    /// `§4`, `new --examples`: every example app the binary carries.
    NewExamples,
    /// `§4.1`: install an app from a zip file on disk or stdin.
    Install { zip: PathBuf, yes: bool },
    /// `§5`.
    Lint {
        paths: Vec<PathBuf>,
        format: Format,
        severity: Severity,
        fix: bool,
    },
    /// `§6`.
    SkillList,
    /// `§6`.
    SkillExport {
        names: Vec<String>,
        out: Option<PathBuf>,
    },
    /// `§7`.
    Snapshot { app: Option<String>, verify: bool },
    /// `§7`.
    Restore {
        from: PathBuf,
        app: Option<String>,
        dry_run: bool,
    },
    /// `§8`.
    Pair {
        open: bool,
        timeout: u64,
        node: bool,
        join: Option<String>,
    },
    /// `§9`.
    Firewall { apply: bool },
}

/// What `parse` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub global: Global,
    pub command: Command,
}

/// A usage error (`§1`, exit 2): what was wrong, in one line. The caller prints [`HELP`]
/// after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

fn usage(message: impl Into<String>) -> Usage {
    Usage(message.into())
}

/// One argument, split at `=` when it is a `--flag=value`.
struct Arg {
    raw: OsString,
}

impl Arg {
    fn text(&self) -> Result<&str, Usage> {
        self.raw
            .to_str()
            .ok_or_else(|| usage(format!("{:?} is not valid Unicode", self.raw)))
    }

    /// `--flag` and an inline `=value`, if this is a flag.
    fn flag(&self) -> Option<(&str, Option<&str>)> {
        let text = self.raw.to_str()?;
        if !text.starts_with('-') || text == "-" || text == "--" {
            return None;
        }
        Some(match text.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (text, None),
        })
    }
}

/// The cursor over the arguments.
struct Args {
    items: Vec<Arg>,
    at: usize,
    /// Everything after a bare `--` is positional.
    literal: bool,
}

impl Args {
    fn peek(&self) -> Option<&Arg> {
        self.items.get(self.at)
    }

    fn next(&mut self) -> Option<&Arg> {
        let item = self.items.get(self.at)?;
        self.at += 1;
        Some(item)
    }

    /// The next argument as a flag, unless it is positional or `--` has been seen.
    fn next_flag(&mut self) -> Option<(String, Option<String>)> {
        if self.literal {
            return None;
        }
        let arg = self.peek()?;
        if arg.raw == "--" {
            self.at += 1;
            self.literal = true;
            return None;
        }
        let (name, value) = arg.flag()?;
        let out = (name.to_owned(), value.map(str::to_owned));
        self.at += 1;
        Some(out)
    }

    /// A flag's value: the inline one, or the next argument.
    fn value(&mut self, name: &str, inline: Option<String>) -> Result<String, Usage> {
        if let Some(value) = inline {
            return Ok(value);
        }
        match self.next() {
            Some(arg) => Ok(arg.text()?.to_owned()),
            None => Err(usage(format!("{name} needs a value"))),
        }
    }

    /// A flag that takes no value.
    fn switch(name: &str, inline: Option<String>) -> Result<(), Usage> {
        match inline {
            None => Ok(()),
            Some(_) => Err(usage(format!("{name} takes no value"))),
        }
    }
}

/// Parse a command line, `argv[0]` excluded.
pub fn parse(argv: impl IntoIterator<Item = OsString>) -> Result<Invocation, Usage> {
    let mut args = Args {
        items: argv.into_iter().map(|raw| Arg { raw }).collect(),
        at: 0,
        literal: false,
    };
    let mut global = Global::default();

    // Global flags, then the command word. `--version` and `--help` end parsing where they
    // stand: `privatium --version dev` is `--version`. A flag that is not global before
    // any command word is the bare command's — `privatium --data-dir d --port 0` — and is
    // handed back to `run` unread.
    let command = loop {
        match args.next_flag() {
            Some((name, inline)) => match global_flag(&mut global, &name, inline, &mut args)? {
                Seen::Terminal(terminal) => {
                    return Ok(Invocation {
                        global,
                        command: terminal,
                    });
                }
                Seen::Global => continue,
                Seen::NotGlobal => {
                    args.at -= 1;
                    break None;
                }
            },
            None => {
                break args
                    .next()
                    .map(|arg| arg.text().map(str::to_owned))
                    .transpose()?;
            }
        }
    };

    let command = match command.as_deref() {
        None => run(&mut global, &mut args)?,
        Some("dev") => dev(&mut global, &mut args)?,
        Some("new") => new(&mut global, &mut args)?,
        Some("install") => install_cmd(&mut global, &mut args)?,
        Some("lint") => lint(&mut global, &mut args)?,
        Some("skill") => skill(&mut global, &mut args)?,
        Some("snapshot") => snapshot(&mut global, &mut args)?,
        Some("restore") => restore(&mut global, &mut args)?,
        Some("pair") => pair(&mut global, &mut args)?,
        Some("firewall") => firewall(&mut global, &mut args)?,
        Some(other) => {
            return Err(usage(format!(
                "{other:?} is not a command; spec/cli.md §10 lists what is deliberately absent"
            )));
        }
    };
    Ok(Invocation { global, command })
}

/// What [`global_flag`] made of a flag.
enum Seen {
    /// A `§1` flag, consumed.
    Global,
    /// `--version` or `--help`, which end parsing.
    Terminal(Command),
    /// Not a `§1` flag; the command's to judge.
    NotGlobal,
}

/// Apply a `§1` flag, if `name` is one.
fn global_flag(
    global: &mut Global,
    name: &str,
    inline: Option<String>,
    args: &mut Args,
) -> Result<Seen, Usage> {
    match name {
        "--data-dir" => global.data_dir = Some(PathBuf::from(args.value(name, inline)?)),
        "--config" => global.config = Some(PathBuf::from(args.value(name, inline)?)),
        "--verbose" => {
            Args::switch(name, inline)?;
            global.verbose = true;
        }
        "--version" | "-V" => {
            Args::switch(name, inline)?;
            return Ok(Seen::Terminal(Command::Version));
        }
        "--help" | "-h" => {
            Args::switch(name, inline)?;
            return Ok(Seen::Terminal(Command::Help));
        }
        _ => return Ok(Seen::NotGlobal),
    }
    Ok(Seen::Global)
}

/// Read a command's flags: each one is either handled by `local`, or is a global flag,
/// or is unknown. Positional arguments go to `positional`.
fn flags(
    global: &mut Global,
    args: &mut Args,
    mut local: impl FnMut(&str, Option<String>, &mut Args) -> Result<bool, Usage>,
    mut positional: impl FnMut(String) -> Result<(), Usage>,
) -> Result<Option<Command>, Usage> {
    loop {
        match args.next_flag() {
            Some((name, inline)) => {
                if local(&name, inline.clone(), args)? {
                    continue;
                }
                match global_flag(global, &name, inline, args)? {
                    Seen::Global => {}
                    Seen::Terminal(terminal) => return Ok(Some(terminal)),
                    Seen::NotGlobal => return Err(usage(format!("unknown flag {name}"))),
                }
            }
            None => match args.next() {
                Some(arg) => positional(arg.text()?.to_owned())?,
                None => return Ok(None),
            },
        }
    }
}

fn port_value(text: &str) -> Result<u16, Usage> {
    text.parse::<u16>()
        .map_err(|_| usage(format!("--port {text:?}: not a port number")))
}

fn no_positional(command: &str) -> impl FnMut(String) -> Result<(), Usage> {
    move |extra| Err(usage(format!("{command}: unexpected argument {extra:?}")))
}

fn run(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let (mut port, mut solo, mut no_discovery, mut open) = (None, None, false, false);
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--port" => port = Some(port_value(&args.value(name, inline)?)?),
                "--solo" => solo = Some(args.value(name, inline)?),
                "--no-discovery" => {
                    Args::switch(name, inline)?;
                    no_discovery = true;
                }
                "--open" => {
                    Args::switch(name, inline)?;
                    open = true;
                }
                _ => return Ok(false),
            }
            Ok(true)
        },
        no_positional("privatium"),
    )?;
    Ok(terminal.unwrap_or(Command::Run {
        port,
        solo,
        no_discovery,
        open,
    }))
}

fn dev(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let (mut app, mut open) = (None, false);
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--app" => app = Some(args.value(name, inline)?),
                "--open" => {
                    Args::switch(name, inline)?;
                    open = true;
                }
                _ => return Ok(false),
            }
            Ok(true)
        },
        no_positional("dev"),
    )?;
    Ok(terminal.unwrap_or(Command::Dev { app, open }))
}

fn new(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let (mut slug, mut tier, mut from, mut scaffold) = (None, None, None, None);
    let mut examples = false;
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--examples" => {
                    Args::switch(name, inline)?;
                    examples = true;
                }
                "--tier" => {
                    let value = args.value(name, inline)?;
                    tier = Some(match value.as_str() {
                        "lua" => Tier::Lua,
                        "web" => Tier::Web,
                        "rust" => Tier::Rust,
                        _ => return Err(usage(format!("--tier {value:?}: lua, web or rust"))),
                    });
                }
                "--from" => from = Some(args.value(name, inline)?),
                "--scaffold" => scaffold = Some(args.value(name, inline)?),
                _ => return Ok(false),
            }
            Ok(true)
        },
        |positional| {
            if slug.is_some() {
                return Err(usage(format!("new: unexpected argument {positional:?}")));
            }
            slug = Some(positional);
            Ok(())
        },
    )?;
    if let Some(terminal) = terminal {
        return Ok(terminal);
    }
    if examples {
        if slug.is_some() || tier.is_some() || from.is_some() || scaffold.is_some() {
            return Err(usage(
                "new --examples: writes every example app under its own slug and takes no \
                 slug and no other flag",
            ));
        }
        return Ok(Command::NewExamples);
    }
    let slug = slug.ok_or_else(|| usage("new: a slug is required, or --examples"))?;
    Ok(Command::New {
        slug,
        tier,
        from,
        scaffold,
    })
}

fn lint(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let mut paths = Vec::new();
    let (mut format, mut severity, mut fix) = (Format::Text, Severity::Info, false);
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--format" => {
                    let value = args.value(name, inline)?;
                    format = match value.as_str() {
                        "text" => Format::Text,
                        "json" => Format::Json,
                        _ => return Err(usage(format!("--format {value:?}: text or json"))),
                    };
                }
                "--severity" => {
                    let value = args.value(name, inline)?;
                    severity = match value.as_str() {
                        "error" => Severity::Error,
                        "warn" => Severity::Warn,
                        "info" => Severity::Info,
                        _ => {
                            return Err(usage(format!(
                                "--severity {value:?}: error, warn or info"
                            )));
                        }
                    };
                }
                "--fix" => {
                    Args::switch(name, inline)?;
                    fix = true;
                }
                _ => return Ok(false),
            }
            Ok(true)
        },
        |positional| {
            paths.push(PathBuf::from(positional));
            Ok(())
        },
    )?;
    Ok(terminal.unwrap_or(Command::Lint {
        paths,
        format,
        severity,
        fix,
    }))
}

fn skill(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    // The subcommand word may be preceded by global flags, as anywhere else.
    let word = loop {
        match args.next_flag() {
            Some((name, inline)) => match global_flag(global, &name, inline, args)? {
                Seen::Global => {}
                Seen::Terminal(terminal) => return Ok(terminal),
                Seen::NotGlobal => return Err(usage(format!("unknown flag {name}"))),
            },
            None => {
                break args
                    .next()
                    .map(|arg| arg.text().map(str::to_owned))
                    .transpose()?;
            }
        }
    };
    match word.as_deref() {
        Some("list") => {
            let terminal = flags(
                global,
                args,
                |_, _, _| Ok(false),
                no_positional("skill list"),
            )?;
            Ok(terminal.unwrap_or(Command::SkillList))
        }
        Some("export") => {
            let mut names = Vec::new();
            let mut out = None;
            let terminal = flags(
                global,
                args,
                |name, inline, args| {
                    if name == "--out" {
                        out = Some(PathBuf::from(args.value(name, inline)?));
                        return Ok(true);
                    }
                    Ok(false)
                },
                |positional| {
                    names.push(positional);
                    Ok(())
                },
            )?;
            Ok(terminal.unwrap_or(Command::SkillExport { names, out }))
        }
        Some(other) => Err(usage(format!("skill: {other:?} is not list or export"))),
        None => Err(usage("skill: list or export is required")),
    }
}

fn snapshot(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let (mut app, mut verify) = (None, false);
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--app" => app = Some(args.value(name, inline)?),
                "--verify" => {
                    Args::switch(name, inline)?;
                    verify = true;
                }
                _ => return Ok(false),
            }
            Ok(true)
        },
        no_positional("snapshot"),
    )?;
    Ok(terminal.unwrap_or(Command::Snapshot { app, verify }))
}

fn restore(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let (mut from, mut app, mut dry_run) = (None, None, false);
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--from" => from = Some(PathBuf::from(args.value(name, inline)?)),
                "--app" => app = Some(args.value(name, inline)?),
                "--dry-run" => {
                    Args::switch(name, inline)?;
                    dry_run = true;
                }
                _ => return Ok(false),
            }
            Ok(true)
        },
        no_positional("restore"),
    )?;
    if let Some(terminal) = terminal {
        return Ok(terminal);
    }
    let from = from.ok_or_else(|| usage("restore: --from <path> is required"))?;
    Ok(Command::Restore { from, app, dry_run })
}

fn pair(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let (mut open, mut timeout, mut node, mut join) = (false, 120u64, false, None);
    let terminal = flags(
        global,
        args,
        |name, inline, args| {
            match name {
                "--open" => {
                    Args::switch(name, inline)?;
                    open = true;
                }
                "--timeout" => {
                    let value = args.value(name, inline)?;
                    timeout = value
                        .parse()
                        .map_err(|_| usage(format!("--timeout {value:?}: seconds")))?;
                }
                "--node" => {
                    Args::switch(name, inline)?;
                    node = true;
                }
                "--join" => join = Some(args.value(name, inline)?),
                _ => return Ok(false),
            }
            Ok(true)
        },
        no_positional("pair"),
    )?;
    if let Some(terminal) = terminal {
        return Ok(terminal);
    }
    if join.is_some() && (node || open) {
        return Err(usage(
            "pair: --join is this space's half of joining; --node and --open belong to the other space's",
        ));
    }
    Ok(Command::Pair {
        open,
        timeout,
        node,
        join,
    })
}

fn install_cmd(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let mut zip: Option<PathBuf> = None;
    let mut yes = false;
    let terminal = flags(
        global,
        args,
        |name, inline, _args| match name {
            "-y" | "--yes" => {
                Args::switch(name, inline)?;
                yes = true;
                Ok(true)
            }
            _ => Ok(false),
        },
        |positional| {
            if zip.is_some() {
                return Err(usage(format!(
                    "install: unexpected argument {positional:?}"
                )));
            }
            zip = Some(PathBuf::from(positional));
            Ok(())
        },
    )?;
    if let Some(terminal) = terminal {
        return Ok(terminal);
    }
    let zip = zip.ok_or_else(|| usage("install: a zip file path is required"))?;
    Ok(Command::Install { zip, yes })
}

fn firewall(global: &mut Global, args: &mut Args) -> Result<Command, Usage> {
    let mut apply = false;
    let terminal = flags(
        global,
        args,
        |name, inline, _| {
            if name == "--apply" {
                Args::switch(name, inline)?;
                apply = true;
                return Ok(true);
            }
            Ok(false)
        },
        no_positional("firewall"),
    )?;
    Ok(terminal.unwrap_or(Command::Firewall { apply }))
}

// AGENTS.md, Style: unwrap() is permitted in tests.
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Result<Invocation, Usage> {
        parse(line.split_whitespace().map(OsString::from))
    }

    #[test]
    fn bare_and_run_flags() {
        let inv = parsed("").unwrap();
        assert_eq!(
            inv.command,
            Command::Run {
                port: None,
                solo: None,
                no_discovery: false,
                open: false
            }
        );
        let inv = parsed("--data-dir /d --port=9000 --solo hello --no-discovery --open").unwrap();
        assert_eq!(
            inv.global.data_dir.as_deref(),
            Some(std::path::Path::new("/d"))
        );
        assert_eq!(
            inv.command,
            Command::Run {
                port: Some(9000),
                solo: Some("hello".into()),
                no_discovery: true,
                open: true
            }
        );
    }

    #[test]
    fn global_flags_anywhere_and_terminals_win() {
        let inv = parsed("dev --app hello --verbose --config c.toml").unwrap();
        assert!(inv.global.verbose);
        assert_eq!(
            inv.global.config.as_deref(),
            Some(std::path::Path::new("c.toml"))
        );
        assert_eq!(
            inv.command,
            Command::Dev {
                app: Some("hello".into()),
                open: false
            }
        );
        assert_eq!(parsed("--version dev").unwrap().command, Command::Version);
        assert_eq!(parsed("new -h").unwrap().command, Command::Help);
    }

    #[test]
    fn every_command_parses_its_synopsis() {
        assert_eq!(
            parsed("new meds --tier web --from hello --scaffold fill")
                .unwrap()
                .command,
            Command::New {
                slug: "meds".into(),
                tier: Some(Tier::Web),
                from: Some("hello".into()),
                scaffold: Some("fill".into())
            }
        );
        assert_eq!(
            parsed("install app.zip --yes").unwrap().command,
            Command::Install {
                zip: "app.zip".into(),
                yes: true,
            }
        );
        assert_eq!(
            parsed("install -y app.zip").unwrap().command,
            Command::Install {
                zip: "app.zip".into(),
                yes: true,
            }
        );
        assert_eq!(
            parsed("lint a b --format json --severity warn --fix")
                .unwrap()
                .command,
            Command::Lint {
                paths: vec!["a".into(), "b".into()],
                format: Format::Json,
                severity: Severity::Warn,
                fix: true
            }
        );
        assert_eq!(
            parsed("new --examples").unwrap().command,
            Command::NewExamples
        );
        assert_eq!(parsed("skill list").unwrap().command, Command::SkillList);
        assert_eq!(
            parsed("skill export a b --out d").unwrap().command,
            Command::SkillExport {
                names: vec!["a".into(), "b".into()],
                out: Some("d".into())
            }
        );
        assert_eq!(
            parsed("snapshot --app hello --verify").unwrap().command,
            Command::Snapshot {
                app: Some("hello".into()),
                verify: true
            }
        );
        assert_eq!(
            parsed("restore --from b --app hello --dry-run")
                .unwrap()
                .command,
            Command::Restore {
                from: "b".into(),
                app: Some("hello".into()),
                dry_run: true
            }
        );
        assert_eq!(
            parsed("pair --open --timeout 30").unwrap().command,
            Command::Pair {
                open: true,
                timeout: 30,
                node: false,
                join: None
            }
        );
        assert_eq!(
            parsed("pair --node --timeout 90").unwrap().command,
            Command::Pair {
                open: false,
                timeout: 90,
                node: true,
                join: None
            }
        );
        assert_eq!(
            parsed("pair --join http://192.0.2.5:8420").unwrap().command,
            Command::Pair {
                open: false,
                timeout: 120,
                node: false,
                join: Some("http://192.0.2.5:8420".into())
            }
        );
        assert_eq!(
            parsed("firewall --apply").unwrap().command,
            Command::Firewall { apply: true }
        );
    }

    #[test]
    fn usage_errors_name_the_problem() {
        for (line, needle) in [
            ("--nope", "--nope"),
            ("new", "slug"),
            ("new a b", "unexpected"),
            ("new a --tier perl", "--tier"),
            ("new a --examples", "--examples"),
            ("new --examples --tier web", "--examples"),
            ("restore", "--from"),
            ("install", "zip"),
            ("install a.zip b.zip", "unexpected"),
            ("skill", "list or export"),
            ("skill nope", "nope"),
            ("--port x", "--port"),
            ("--port", "value"),
            ("doctor", "doctor"),
            ("serve", "serve"),
            ("--open=1", "no value"),
            ("pair --join http://x --node", "--join"),
            ("pair --join http://x --open", "--join"),
            ("pair --join", "value"),
        ] {
            let error = parsed(line).unwrap_err();
            assert!(error.0.contains(needle), "{line:?}: {error}");
        }
    }
}
