// This file is part of Privatium
// crates/privatium/src/main.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-08-31
// Last Modified: 2026-09-26
// Summary: Entry point: spec/cli.md. Bare `privatium` runs a node; `dev`, `new`, `lint`, `skill`,
//          `snapshot`, `restore` and `pair` are the subcommands this build has; `firewall` is
//          not built yet, and it parses and says so rather than being absent, so the help text
//          matches the spec. Exit codes are §1's. Errors are anyhow at this boundary
//          (AGENTS.md, Style) and print as one line.
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

use std::process::ExitCode;

mod cli;
mod data;
mod lint;
mod new;
mod node;
mod pair;
mod run;
mod skill;

use cli::Command;

/// The protocol claim of `spec/cli.md §1`: a build that does not satisfy every item of
/// `spec/protocol.md §13` qualifies the string rather than printing a bare `pv/1`. What
/// this build cannot claim is sync and the remote transports (`spec/protocol.md §10`,
/// `§11`). The wire format itself is `privatium_core::PROTOCOL`.
fn protocol_claim() -> String {
    format!("{} (partial: phase 2)", privatium_core::PROTOCOL)
}

/// `--version` (`spec/cli.md §1`): the build version and the protocol claim on the first
/// line, then the project's own facts — product, author, copyright, licence and the two
/// URLs. The version is the workspace's, in the short form the release tags use
/// (`privatium_core::VERSION`). Every value below the first line comes from
/// `[workspace.package]` through the `CARGO_PKG_*` variables, or from `build.rs` for the
/// two Cargo has no field for, so nothing here is a second copy of a name or a licence.
fn version_line() -> String {
    format!(
        "privatium {version} {protocol}\n\
         {description}\n\
         Product:       Privatium\n\
         Author:        {authors}\n\
         Copyright:     © {year} {holder}\n\
         Licence:       {license} — see main README.md for full license information.\n\
         Project:       {repository}\n\
         Author's site: {author_url}",
        version = privatium_core::VERSION,
        protocol = protocol_claim(),
        description = env!("CARGO_PKG_DESCRIPTION"),
        authors = env!("CARGO_PKG_AUTHORS"),
        year = env!("PV_COPYRIGHT_YEAR"),
        holder = env!("PV_COPYRIGHT_HOLDER"),
        license = env!("CARGO_PKG_LICENSE"),
        repository = env!("CARGO_PKG_REPOSITORY"),
        author_url = env!("PV_AUTHOR_URL"),
    )
}

fn main() -> ExitCode {
    let invocation = match cli::parse(std::env::args_os().skip(1)) {
        Ok(invocation) => invocation,
        Err(usage) => {
            eprintln!("privatium: {usage}\n\n{}", cli::HELP);
            return ExitCode::from(2);
        }
    };

    let outcome = match invocation.command {
        Command::Version => {
            println!("{}", version_line());
            Ok(0)
        }
        Command::Help => {
            print!("{}", cli::HELP);
            Ok(0)
        }
        Command::Run {
            port,
            solo,
            no_discovery,
            open,
        } => run::run(
            &invocation.global,
            run::Options {
                port,
                solo,
                no_discovery,
                open,
                dev_app: None,
                dev: false,
            },
        ),
        Command::Dev { app, open } => run::run(
            &invocation.global,
            run::Options {
                port: None,
                solo: None,
                no_discovery: false,
                open,
                dev_app: app,
                dev: true,
            },
        ),
        Command::New {
            slug,
            tier,
            from,
            scaffold,
        } => new::new(
            &invocation.global,
            &slug,
            tier,
            from.as_deref(),
            scaffold.as_deref(),
        ),
        Command::NewExamples => new::examples(&invocation.global),
        Command::Lint {
            paths,
            format,
            severity,
            fix,
        } => lint::lint(&invocation.global, &paths, format, severity, fix),
        Command::SkillList => skill::list(),
        Command::SkillExport { names, out } => skill::export(&names, out.as_deref()),
        Command::Snapshot { app, verify } => {
            data::snapshot(&invocation.global, app.as_deref(), verify)
        }
        Command::Restore { from, app, dry_run } => {
            data::restore(&invocation.global, &from, app.as_deref(), dry_run)
        }
        Command::Pair {
            open,
            timeout,
            node,
            join,
        } => match join {
            Some(url) => pair::join(&invocation.global, &url),
            None => pair::pair(&invocation.global, open, timeout, node),
        },
        Command::Firewall { .. } => not_in_this_build(
            "firewall",
            "the firewall helper is Phase 6 of docs/roadmap.md; spec/cli.md §9 is its contract",
        ),
    };

    match outcome {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("privatium: {error:#}");
            ExitCode::from(1)
        }
    }
}

/// A command the spec has and this build does not: it parses, so the help text is the
/// spec's, and it says exactly why it stops.
fn not_in_this_build(command: &str, why: &str) -> anyhow::Result<u8> {
    eprintln!("privatium {command}: not in this build — {why}");
    Ok(1)
}
