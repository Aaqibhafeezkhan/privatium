<!--
This file is part of Privatium
docs/phase-prompt-template.md
Author(s): Gabriel Mongefranco
Created: 2026-09-05
Last Modified: 2026-09-26
Summary: A copy-and-paste prompt for starting a Privatium phase, milestone, change or repair in a new
         AI chat. Non-normative.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Phase Prompt Template

## A consistent way to begin each phase in a new AI chat

[Return to the README](../README.md)

Use this prompt when you start a new chat for Privatium work. It tells the agent what the
project is, where its current truth lives, what to read and in which order, and which
rules must hold across the chat boundary. Your request goes in one clearly marked place
at the end.

The template works for a whole phase, one milestone within a phase, a repair, a
documentation change, or any other bounded request. The repository and Git are the
handoff between chats; the new agent should never need the previous transcript.

## How to use this template

1. Start a new chat with an agent that can read and edit the Privatium repository.
2. Copy the complete prompt below.
3. Replace `[ENTER YOUR REQUEST HERE]` with your request.
4. Name the phase and milestone when you know them. For example: "Implement M14 of
   `docs/plans/phase-2.md`."
5. Say whether the agent may commit, push, or open a pull request. If you do not say so,
   the agent leaves the verified changes uncommitted.

## Copy-and-paste prompt

```text
You are working on Privatium, a framework for building small, personal applications that
run entirely on the owner's own hardware, are reachable from every device the owner has,
and sync across those devices with no server, no account, no domain and no cloud. A node
is one Rust binary. It stores everything as append-only JSONL event logs — one file per
device, never modified — and materializes them into a disposable SQLite cache; backup is
copying a folder, and restore is copying it back. Apps come in three tiers by language —
Lua with server-rendered templates, the author's own web front end against a data API,
or Rust linking the core crate — and none has a ceiling. The owner is often not a
professional developer and frequently works with an AI assistant, which is why the
repository ships skills for assistants and a linter that makes them enforceable.

This is a specification-first project: spec/ is normative, docs/ explains, apps/ holds
the reference apps, and where the code and the spec disagree the spec wins and the code
is wrong.

Read these files before changing anything, in this order:

1. AGENTS.md — the highest local instruction source. Its twelve invariants are
   non-negotiable; its "things agents get wrong here" is a list of mistakes already made
   once. Follow its style, security, personal-data, accessibility, testing, writing,
   licensing, change-discipline and response-format rules exactly.

2. README.md — the user promise, the quick start, the current status, the table of
   documents, the credits, and the copyright, trademark, licence and citation
   boilerplate, which is preserved exactly.

3. docs/architecture.md — the six decisions everything follows from, the component map,
   the client tiers, clusters, the data flows, and what is deliberately absent.

4. docs/roadmap.md — the phases, each with its deliverable and its acceptance bullets.
   Find the phase my request belongs to and confirm it is not already complete.

5. The plan for that phase under docs/plans.

6. If my request touches code: spec/protocol.md and spec/app-contract.md, in full, and
   then whichever of spec/lua-api.md, spec/data-api.md, spec/data-dictionary.md and
   spec/cli.md the request concerns. They are the contract.

7. The source files, tests, apps, skills and documents directly related to my request,
   and the decision records under docs/decisions/ they cite.

Before writing:

- Run git status; inspect the current branch and the recent commits that touch what I
  am asking about. Preserve unrelated and uncommitted work.
- Compare my request with the roadmap, the plan, the spec and the repository as it
  stands. If the request is already done, say so.
- If the request conflicts with a settled rule, an invariant, or a decided plan
  section, or if it needs a decision that would change the architecture, the security
  posture, or how data is stored, shared or identified, explain the conflict and ask
  before changing anything. Otherwise make the safest reasonable assumption, state it in
  one line, and continue.
- Give me a short breakdown of the work, the tests that will hold it, and the important
  assumptions. Continue without another approval unless a decision, a permission or a
  risk needs me.

While working:

- Stay within the requested phase or milestone. One milestone is one branch and one PR;
  do not quietly begin the next.
- When implementation reveals a spec defect, fix the spec in the same change and record
  it in the plan's §3. Never implement around the spec and never code around a gap.
- A change to spec/ that is not reflected in app-skills/ is incomplete: regenerate with
  `cargo xtask gen-skill-reference` and edit the SKILL.md files that cite what changed.
- Write the named tests first. Every normative MUST maps to a named test carrying the
  spec's section number; cover the empty input, the missing configuration, the invalid
  value, the boundary and the unauthorized caller.
- Reuse what exists. No new crate without a stated reason, a maintenance check and a
  clean `cargo deny`; no dependency the plan's dependency table does not name without
  saying why.
- Keep Linux, macOS and Windows working. CI runs Linux on a pull request and all three
  on the merge to `main`; the `ci-full-matrix` label asks for all three on the pull
  request, which is what a change to a path, a process, a file lock or a C dependency
  wants.
- Everything from outside the process is untrusted. Fail closed. Parameterize SQL.
  Escape output. Vetted cryptography only.
- No secret and no personal data in a log, an error, a comment, a test fixture or a
  commit. Synthetic examples only.
- Comments are timeless and cite documents by section. Never name this chat, a
  milestone number, a plan step or the agent as the reason for code.
- Every source file carries the header block; `Modified:` moves on a material change.

Before finishing:

- Run the targeted tests as you go, then the gates: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`, `cargo xtask header-check`,
  `cargo xtask gen-skill-reference --check`, `cargo xtask lint-spec-refs`,
  `privatium lint` over every app folder touched, and `node --test` over
  crates/privatium-core/tests/js/ when pv.js or the client changed.
- Review the change for security, personal data, accessibility (the PV4xx rules and
  tests/common/a11y.rs, plus the manual pass AGENTS.md requires for anything a person
  reads or operates), documentation, licensing and cross-platform effects.
- Update docs/ in the same change whenever behaviour, configuration, a schema, a
  default, an error message, or the security or accessibility posture changed. Planned
  behaviour is labelled with its phase.
- Record unfinished work, a failed or unavailable check, an accepted risk or a deferral
  where it will survive this chat: the plan, the roadmap, or an issue — never only here.
- At a milestone boundary, tick the plan's checklist and the roadmap's bullets only for
  what is green, naming the test that holds each.
- Do not commit, push, open a pull request, merge, tag or release unless my request
  authorizes it. Never force-push or rewrite shared history.
- Finish with the response format AGENTS.md prescribes for a code change: files changed,
  the security and accessibility reviews where they apply, the exact verification
  commands and their outcomes or "not executed", the documents changed, the assumptions
  that matter, and a two-to-four-sentence summary that says what I do next.

My request:
[ENTER YOUR REQUEST HERE]
```

## Example requests

```text
My request:
Implement M14 of docs/plans/phase-2.md on a branch named m14-cluster-identity. Commit
there when every gate passes; do not push.
```

```text
My request:
The devices page shows a paired phone twice after a restart. Find the cause, fix it with
a test that would have caught it, and leave the change uncommitted.
```

```text
My request:
Write docs/plans/phase-4.md from docs/roadmap.md Phase 4, in the shape of
docs/plans/phase-1.md. Documentation only; no code.
```

## Conclusion

A fresh chat begins with the repository, not a retelling of the last one. This template
gives every agent the same description of the project, the same reading order, the same
boundaries and the same finishing rules, and leaves one place for the work you want done.

## Additional resources

- [README](../README.md)
- [Repository instructions](../AGENTS.md)
- [Architecture](architecture.md)
- [Roadmap](roadmap.md)
- [Plans](plans/)
- [Backup and restore](backup-and-restore.md)

[Return to the README](../README.md)

---

Copyright © 2026 Gabriel Mongefranco
