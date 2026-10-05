// This file is part of Privatium
// crates/privatium-core/tests/lint.rs
// Author(s): Gabriel Mongefranco
// Created: 2026-09-05
// Last Modified: 2026-10-04
// Summary: The linter against its corpus (spec/cli.md §5.4): every rule's pass fixture is clean and its
//          fail fixture trips the rule, a rule with no pair fails the suite, the reference
//          apps lint clean, every finding's spec reference resolves against this checkout
//          (§5.2), the JSON carries the seven fields, --fix is mechanical and touches nothing
//          else (§5.3), the scaffold's output is clean (§4), and PV404's unit is the page as
//          rendered (§5.1).
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

// AGENTS.md, Style: unwrap() is permitted in tests, and a test that hides a failure
// behind `?` is worse than one that panics with a line number.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use privatium_core::app::manifest::Tier;
use privatium_core::app::scaffold::{self, File};
use privatium_core::config::Config;
use privatium_core::lint::{self, Depth, Finding, Options, RULES, RuleId};
use privatium_core::{Schema, lint::spec_ref};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn corpus(kind: &str, id: RuleId) -> PathBuf {
    repo()
        .join("apps")
        .join("_lint")
        .join(kind)
        .join(id.as_str())
}

/// The node configuration a fixture is linted under: the rule directory's `config.toml`,
/// or the defaults (host mode).
fn options_for(rule_dir: &Path) -> Options {
    let config = rule_dir.join("config.toml");
    if config.is_file() {
        Options::from_config(&Config::load(&config).unwrap())
    } else {
        Options::default()
    }
}

fn lint_corpus(kind: &str, id: RuleId) -> Vec<Finding> {
    let dir = corpus(kind, id);
    let apps = lint::discover(&dir, Depth::Any);
    assert!(!apps.is_empty(), "{} holds no app", dir.display());
    let options = options_for(&dir);
    apps.iter()
        .flat_map(|app| {
            let display = format!(
                "apps/_lint/{kind}/{}/{}",
                id.as_str(),
                app.file_name().unwrap().to_string_lossy()
            );
            lint::lint_app(app, &display, &options)
        })
        .collect()
}

fn ids(findings: &[Finding]) -> BTreeSet<RuleId> {
    findings.iter().map(|f| f.id).collect()
}

fn describe(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(Finding::text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The pair of tests each rule gets over the corpus (`spec/cli.md §5.4`: a rule without a
/// passing and a failing case is not implemented). The list is checked against `RULES`
/// by `test_every_rule_has_fixtures`, so a new rule without its two tests fails there.
macro_rules! rule_tests {
    ($($id:ident => ($passes:ident, $fails:ident)),* $(,)?) => {
        const TESTED: &[RuleId] = &[$(RuleId::$id),*];
        $(
            #[test]
            fn $passes() {
                let findings = lint_corpus("pass", RuleId::$id);
                assert!(findings.is_empty(), "the pass fixture of {} is not clean:\n{}", RuleId::$id, describe(&findings));
            }

            #[test]
            fn $fails() {
                let findings = lint_corpus("fail", RuleId::$id);
                assert!(ids(&findings).contains(&RuleId::$id), "the fail fixture of {} did not trip it; found:\n{}", RuleId::$id, describe(&findings));
                for finding in &findings {
                    assert_eq!(finding.spec, lint::rule(finding.id).spec);
                    assert_eq!(finding.severity, lint::rule(finding.id).severity);
                }
            }
        )*
    };
}

rule_tests! {
    PV101 => (test_lint_rule_pv101_passes, test_lint_rule_pv101_fails),
    PV102 => (test_lint_rule_pv102_passes, test_lint_rule_pv102_fails),
    PV103 => (test_lint_rule_pv103_passes, test_lint_rule_pv103_fails),
    PV104 => (test_lint_rule_pv104_passes, test_lint_rule_pv104_fails),
    PV105 => (test_lint_rule_pv105_passes, test_lint_rule_pv105_fails),
    PV106 => (test_lint_rule_pv106_passes, test_lint_rule_pv106_fails),
    PV107 => (test_lint_rule_pv107_passes, test_lint_rule_pv107_fails),
    PV108 => (test_lint_rule_pv108_passes, test_lint_rule_pv108_fails),
    PV109 => (test_lint_rule_pv109_passes, test_lint_rule_pv109_fails),
    PV110 => (test_lint_rule_pv110_passes, test_lint_rule_pv110_fails),
    PV201 => (test_lint_rule_pv201_passes, test_lint_rule_pv201_fails),
    PV202 => (test_lint_rule_pv202_passes, test_lint_rule_pv202_fails),
    PV203 => (test_lint_rule_pv203_passes, test_lint_rule_pv203_fails),
    PV204 => (test_lint_rule_pv204_passes, test_lint_rule_pv204_fails),
    PV205 => (test_lint_rule_pv205_passes, test_lint_rule_pv205_fails),
    PV206 => (test_lint_rule_pv206_passes, test_lint_rule_pv206_fails),
    PV207 => (test_lint_rule_pv207_passes, test_lint_rule_pv207_fails),
    PV208 => (test_lint_rule_pv208_passes, test_lint_rule_pv208_fails),
    PV301 => (test_lint_rule_pv301_passes, test_lint_rule_pv301_fails),
    PV302 => (test_lint_rule_pv302_passes, test_lint_rule_pv302_fails),
    PV303 => (test_lint_rule_pv303_passes, test_lint_rule_pv303_fails),
    PV304 => (test_lint_rule_pv304_passes, test_lint_rule_pv304_fails),
    PV305 => (test_lint_rule_pv305_passes, test_lint_rule_pv305_fails),
    PV306 => (test_lint_rule_pv306_passes, test_lint_rule_pv306_fails),
    PV307 => (test_lint_rule_pv307_passes, test_lint_rule_pv307_fails),
    PV308 => (test_lint_rule_pv308_passes, test_lint_rule_pv308_fails),
    PV401 => (test_lint_rule_pv401_passes, test_lint_rule_pv401_fails),
    PV402 => (test_lint_rule_pv402_passes, test_lint_rule_pv402_fails),
    PV403 => (test_lint_rule_pv403_passes, test_lint_rule_pv403_fails),
    PV404 => (test_lint_rule_pv404_passes, test_lint_rule_pv404_fails),
    PV405 => (test_lint_rule_pv405_passes, test_lint_rule_pv405_fails),
    PV406 => (test_lint_rule_pv406_passes, test_lint_rule_pv406_fails),
    PV407 => (test_lint_rule_pv407_passes, test_lint_rule_pv407_fails),
    PV408 => (test_lint_rule_pv408_passes, test_lint_rule_pv408_fails),
    PV501 => (test_lint_rule_pv501_passes, test_lint_rule_pv501_fails),
    PV502 => (test_lint_rule_pv502_passes, test_lint_rule_pv502_fails),
    PV503 => (test_lint_rule_pv503_passes, test_lint_rule_pv503_fails),
    PV504 => (test_lint_rule_pv504_passes, test_lint_rule_pv504_fails),
    PV505 => (test_lint_rule_pv505_passes, test_lint_rule_pv505_fails),
    PV506 => (test_lint_rule_pv506_passes, test_lint_rule_pv506_fails),
}

/// The meta-test: every rule has a pass and a fail fixture, and the pair of tests above.
#[test]
fn test_every_rule_has_fixtures() {
    let tested: BTreeSet<RuleId> = TESTED.iter().copied().collect();
    for rule in RULES {
        assert!(
            tested.contains(&rule.id),
            "{} has no rule_tests! entry",
            rule.id
        );
        for kind in ["pass", "fail"] {
            let dir = corpus(kind, rule.id);
            assert!(
                dir.is_dir(),
                "{} has no {kind} fixture at {}",
                rule.id,
                dir.display()
            );
            assert!(
                !lint::discover(&dir, Depth::Any).is_empty(),
                "{} holds no app",
                dir.display()
            );
        }
    }
    assert_eq!(tested.len(), RULES.len());
}

/// `spec/cli.md §5.4` — no stray fixture: every directory under the corpus is a rule.
#[test]
fn test_spec_cli_5_4_lint_corpus_files_all_belong_to_a_rule() {
    for kind in ["pass", "fail"] {
        let dir = repo().join("apps").join("_lint").join(kind);
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                RuleId::parse(&name).is_some(),
                "{kind}/{name} is not a rule of spec/cli.md §5.1"
            );
        }
    }
}

/// `spec/cli.md §5.4` — the reference apps are the corpus's clean end.
#[test]
fn test_reference_apps_lint_clean() {
    for slug in ["hello", "animals", "sketch", "pantry"] {
        let dir = repo().join("apps").join(slug);
        let findings = lint::lint_app(&dir, &format!("apps/{slug}"), &Options::default());
        assert!(findings.is_empty(), "apps/{slug}:\n{}", describe(&findings));
    }
}

/// `spec/cli.md §5.2` — every rule cites a document and section this checkout has, and
/// every finding over the corpus carries its rule's reference.
#[test]
fn test_every_finding_has_resolvable_spec_ref() {
    let failures = spec_ref::check_rules(&repo());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for rule in RULES {
        for finding in lint_corpus("fail", rule.id) {
            assert_eq!(finding.spec, lint::rule(finding.id).spec);
            spec_ref::resolve(&repo(), finding.spec).unwrap();
        }
    }
}

/// `spec/cli.md §5.2` — one object per finding, the seven fields, resolvable spec.
#[test]
fn test_spec_cli_5_2_json_findings_carry_seven_fields() {
    let findings = lint_corpus("fail", RuleId::PV301);
    assert!(!findings.is_empty());
    for finding in &findings {
        let value: serde_json::Value = serde_json::from_str(&finding.json()).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 7, "{value}");
        for key in ["id", "severity", "file", "line", "message", "fix", "spec"] {
            assert!(object.contains_key(key), "{key} missing from {value}");
        }
        assert_eq!(value["id"], "PV301");
        assert_eq!(value["severity"], "error");
        assert_eq!(value["spec"], "spec/app-contract.md §2.2");
        assert!(value["fix"].as_str().unwrap().contains("url("), "{value}");
        assert!(
            value["file"]
                .as_str()
                .unwrap()
                .starts_with("apps/_lint/fail/PV301/")
        );
        assert!(value["line"].as_u64().unwrap() > 0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// `spec/cli.md §5.3` — `--fix` rewrites a literal mount path to `url()` and adds
/// `focusable="false"` to an inline icon, and touches nothing else: SQL and Lua control
/// flow stay as written.
#[test]
fn test_spec_cli_5_3_fix_is_mechanical_only() {
    let scratch = tempfile::tempdir().unwrap();
    for (rule, slug) in [
        (RuleId::PV301, "pv301bad"),
        (RuleId::PV401, "pv401bad"),
        (RuleId::PV303, "pv303bad"),
    ] {
        copy_dir(&corpus("fail", rule).join(slug), &scratch.path().join(slug));
    }
    let lint_all = || -> Vec<Finding> {
        ["pv301bad", "pv401bad", "pv303bad"]
            .iter()
            .flat_map(|slug| lint::lint_app(&scratch.path().join(slug), slug, &Options::default()))
            .collect()
    };
    let before = lint_all();
    let before_303 = fs::read_to_string(scratch.path().join("pv303bad").join("app.lua")).unwrap();
    let written = lint::apply(&before).unwrap();
    assert_eq!(written.len(), 3, "{written:?}");
    let after = lint_all();

    let lua = fs::read_to_string(scratch.path().join("pv301bad").join("app.lua")).unwrap();
    assert!(lua.contains("pv.redirect(url('/'))"), "{lua}");
    let view = fs::read_to_string(
        scratch
            .path()
            .join("pv301bad")
            .join("views")
            .join("index.lsp"),
    )
    .unwrap();
    assert!(view.contains("href=\"<?= url('/edit') ?>\""), "{view}");
    assert!(
        !after.iter().any(|f| f.id == RuleId::PV301),
        "{}",
        describe(&after)
    );

    let icon_view = fs::read_to_string(
        scratch
            .path()
            .join("pv401bad")
            .join("views")
            .join("index.lsp"),
    )
    .unwrap();
    assert!(
        icon_view.contains("<svg focusable=\"false\" aria-hidden=\"true\""),
        "{icon_view}"
    );
    let remaining_401: Vec<&Finding> = after.iter().filter(|f| f.id == RuleId::PV401).collect();
    assert_eq!(
        remaining_401.len(),
        1,
        "the icon-only control is not mechanical:\n{}",
        describe(&after)
    );
    assert!(remaining_401[0].message.contains("icon-only"));

    // Never SQL, never Lua control flow.
    let after_303 = fs::read_to_string(scratch.path().join("pv303bad").join("app.lua")).unwrap();
    assert_eq!(before_303, after_303);
    assert!(after.iter().any(|f| f.id == RuleId::PV303));
}

fn write_files(dir: &Path, files: &[File]) {
    for file in files {
        let path = dir.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
}

/// `spec/cli.md §4` — what `privatium new` writes is lint-clean: the fresh app of each
/// tier and the CRUD screens for a typed table.
#[test]
fn test_spec_cli_4_scaffold_lints_clean() {
    let scratch = tempfile::tempdir().unwrap();
    for (tier, slug) in [
        (Tier::Lua, "fresh-lua"),
        (Tier::Web, "fresh-web"),
        (Tier::Rust, "fresh-rust"),
    ] {
        let dir = scratch.path().join(slug);
        write_files(&dir, &scaffold::fresh(slug, "Fresh", tier));
        let findings = lint::lint_app(&dir, slug, &Options::default());
        assert!(findings.is_empty(), "{slug}:\n{}", describe(&findings));
    }
    const DDL: &str = "CREATE TABLE fill (
        id     VARCHAR PRIMARY KEY,
        drug   VARCHAR NOT NULL,
        copay  DECIMAL(18,2),
        taken  BOOLEAN,
        on_day DATE,
        tags   VARCHAR[]
    );";
    let dir = scratch.path().join("meds");
    let mut files = scaffold::fresh("meds", "Meds", Tier::Lua);
    files.retain(|f| f.path != "app.lua" && !f.path.starts_with("views/"));
    files.push(File {
        path: "schema.sql".into(),
        contents: DDL.as_bytes().to_vec(),
    });
    files.extend(scaffold::crud(&Schema::parse(DDL).unwrap(), "fill").unwrap());
    write_files(&dir, &files);
    let findings = lint::lint_app(&dir, "meds", &Options::default());
    assert!(findings.is_empty(), "meds:\n{}", describe(&findings));
}

fn app_with(dir: &Path, slug: &str, lua: &str, views: &[(&str, &str)]) {
    fs::create_dir_all(dir.join("views")).unwrap();
    fs::write(
        dir.join("app.toml"),
        format!("[app]\nslug = \"{slug}\"\ntitle = \"T\"\nversion = \"1.0.0\"\napi = 1\ntier = \"lua\"\n"),
    )
    .unwrap();
    fs::write(dir.join("app.lua"), lua).unwrap();
    for (name, body) in views {
        fs::write(dir.join("views").join(format!("{name}.lsp")), body).unwrap();
    }
}

/// `spec/cli.md §5.1` (`PV404`, plan §3 row 68) — the unit is the page as rendered: a
/// view with its partials, or a layout's document; a fragment answering htmx is judged
/// where it lands; every state of a branch supplies the one `<h1>`.
#[test]
fn test_spec_cli_5_1_pv404_unit_is_the_rendered_page() {
    let scratch = tempfile::tempdir().unwrap();
    let pv404 = |findings: &[Finding]| -> Vec<String> {
        findings
            .iter()
            .filter(|f| f.id == RuleId::PV404)
            .map(|f| f.text())
            .collect()
    };
    let lua = "local pv = require 'privatium'\n\
               pv.get('/', function(req)\n  if req.is_htmx then return pv.render('_board', {}) end\n  return pv.render('play', {})\nend)\n";

    // The animals shape: play has no h1, _board has one in every branch.
    let a = scratch.path().join("a");
    app_with(
        &a,
        "a",
        lua,
        &[
            (
                "play",
                "<div id=\"board\"><?= render('_board', {}) ?></div>\n<h2>About</h2>\n",
            ),
            (
                "_board",
                "<? if not node then ?><h1>Empty</h1><? elseif node.q then ?><h1><?= node.q ?></h1><? else ?><h1>Is it?</h1><? end ?>\n",
            ),
        ],
    );
    let findings = lint::lint_app(&a, "a", &Options::default());
    assert!(pv404(&findings).is_empty(), "{}", describe(&findings));

    // play with its own h1 as well: two in every state.
    let b = scratch.path().join("b");
    app_with(
        &b,
        "b",
        lua,
        &[
            (
                "play",
                "<h1>Play</h1><div id=\"board\"><?= render('_board', {}) ?></div>\n",
            ),
            ("_board", "<h1>Board</h1>\n"),
        ],
    );
    let findings = lint::lint_app(&b, "b", &Options::default());
    assert!(
        pv404(&findings).iter().any(|f| f.contains("more than one")),
        "{}",
        describe(&findings)
    );

    // A branch with no h1: the empty state.
    let c = scratch.path().join("c");
    app_with(
        &c,
        "c",
        "local pv = require 'privatium'\npv.get('/', function() return pv.render('index', {}) end)\n",
        &[("index", "<? if rows then ?><h1>Rows</h1><? end ?>\n")],
    );
    let findings = lint::lint_app(&c, "c", &Options::default());
    assert!(
        pv404(&findings).iter().any(|f| f.contains("no <h1>")),
        "{}",
        describe(&findings)
    );

    // A layout owns the document: its h1 plus the view's content is one.
    let d = scratch.path().join("d");
    app_with(
        &d,
        "d",
        "local pv = require 'privatium'\npv.get('/', function() return pv.render('index', {}) end)\n",
        &[
            ("index", "<? layout('base') ?>\n<h2>Section</h2>\n"),
            (
                "base",
                "<!doctype html><html lang=\"en\"><body><h1>Site</h1><main><?= content ?></main></body></html>\n",
            ),
        ],
    );
    let findings = lint::lint_app(&d, "d", &Options::default());
    assert!(pv404(&findings).is_empty(), "{}", describe(&findings));

    // The same layout with a view that brings its own h1: two.
    let e = scratch.path().join("e");
    app_with(
        &e,
        "e",
        "local pv = require 'privatium'\npv.get('/', function() return pv.render('index', {}) end)\n",
        &[
            ("index", "<? layout('base') ?>\n<h1>Page</h1>\n"),
            (
                "base",
                "<!doctype html><html lang=\"en\"><body><h1>Site</h1><main><?= content ?></main></body></html>\n",
            ),
        ],
    );
    let findings = lint::lint_app(&e, "e", &Options::default());
    assert!(
        pv404(&findings).iter().any(|f| f.contains("more than one")),
        "{}",
        describe(&findings)
    );

    // A loop around an h1: any number of them.
    let f = scratch.path().join("f");
    app_with(
        &f,
        "f",
        "local pv = require 'privatium'\npv.get('/', function() return pv.render('index', {}) end)\n",
        &[(
            "index",
            "<? for _, r in ipairs(rows) do ?><h1><?= r.name ?></h1><? end ?>\n",
        )],
    );
    let findings = lint::lint_app(&f, "f", &Options::default());
    assert_eq!(pv404(&findings).len(), 2, "{}", describe(&findings));
}

/// `spec/cli.md §5` — a path may be an app, a folder of apps, or a file inside an app;
/// a path with no app under it is one `PV101` finding.
#[test]
fn test_spec_cli_5_paths_are_apps_folders_or_files() {
    let apps = repo().join("apps");
    let report = lint::lint_paths(&[apps.join("hello")], &Options::default());
    assert_eq!(report.apps.len(), 1);
    assert!(report.findings.is_empty(), "{}", describe(&report.findings));

    let report = lint::lint_paths(&[corpus("fail", RuleId::PV301)], &Options::default());
    assert_eq!(report.apps.len(), 1);
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.file.contains("PV301/pv301bad/")),
        "{}",
        describe(&report.findings)
    );

    let file = corpus("fail", RuleId::PV301)
        .join("pv301bad")
        .join("app.lua");
    let report = lint::lint_paths(&[file], &Options::default());
    assert!(!report.findings.is_empty());
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.file.ends_with("pv301bad/app.lua")),
        "{}",
        describe(&report.findings)
    );

    let empty = tempfile::tempdir().unwrap();
    let report = lint::lint_paths(&[empty.path().to_path_buf()], &Options::default());
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].id, RuleId::PV101);
}
