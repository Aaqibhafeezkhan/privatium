<!--
This file is part of Privatium
docs/app-repository.md
Author(s): Gabriel Mongefranco
Created: 2026-09-26
Last Modified: 2026-09-26
Summary: How to start a Privatium app in a repository of its own: the repository template, the
         app folder, the assistant guides, the framework version pin, and how to run, lint and
         share the app. Ends with a prompt that walks an AI assistant through the same steps.
Notes: See README file for documentation and full license information.

Copyright © 2026 Gabriel Mongefranco

Permission is granted to copy, distribute and/or modify this document
under the terms of the GNU Free Documentation License, Version 1.3 or
any later version published by the Free Software Foundation; with no
Invariant Sections, no Front-Cover Texts, and no Back-Cover Texts.
See <https://www.gnu.org/licenses/fdl-1.3.html>.
-->

# Your app in its own repository

[Back to the project README](../README.md)

This guide is for anyone who wants to keep a Privatium app in a git repository of its own.
It covers creating the repository from a template and putting the app folder in it. It then
covers the assistant guides, the framework version pin, and how to run and share the app.
You can follow it by hand, or hand the prompt in section 10 to an AI assistant and let it
do the work.

An app is a folder with an `app.toml` in it. That is the whole contract with the framework,
so a repository adds nothing the node needs. It adds what you and your collaborators need: a
README, a license, a citation, an issue tracker, releases, and a place for the assistant
guides that match the framework version you run.

## 1. Before you start

You need three things on your computer.

1. **The Privatium program.** Download the release for your platform from the
   [Privatium README](../README.md), and note its version. Every command below assumes
   `privatium` is on your path or in the current folder.
2. **Git, and a GitHub account.** The template lives on GitHub. The
   [GitHub command-line tool](https://cli.github.com/) makes one step shorter but is optional.
3. **A decision about the tier.** Records, lists, forms and trackers are Tier 1, written in
   Lua. Read the [overview guide](../skills/privatium-overview/SKILL.md) if you are not
   sure. This page uses Tier 1 in its examples, and the steps are the same for the other
   tiers.

## 2. Create the repository from the template

The [repository template](https://github.com/gabrielmongefranco/repo-template) carries the
files every project of this author's has: a README, a license, a citation file, agent
instructions, a documentation folder and four general-purpose skills.

1. On the template's page, click **Use this template**, name the new repository and clone
   it. With the command-line tool, one command does all three:

   ```sh
   gh repo create gabrielmongefranco/privatium-example-tracker \
     --template gabrielmongefranco/repo-template --public --clone
   ```

2. Open the new repository's `README.md` and follow the setup block at the top. It asks for
   a find-and-replace across every file for four placeholders:

   | Placeholder | Replace with | Example |
   |---|---|---|
   | `YOUR_PROJECT_TITLE` | The app's title | `Example Tracker` |
   | `YOUR_REPO_NAME` | The repository name, used in URLs | `privatium-example-tracker` |
   | `YOUR_YEAR` | The current year | `2026` |
   | `YOUR_DOI` | A DOI, or delete it until you have one | `10.5281/zenodo.0000000` |

3. Edit by hand the parts the block lists: the description and credits in `README.md`,
   the `authors` block in `CITATION.cff`, and the `creators` block in `.zenodo.json`.
   Under credits, list Privatium as a project this work is based on, with its license,
   GPL-3.0-or-later, and a link to its repository.

4. Delete the setup block, and keep the template attribution line below it.

The template's `AGENTS.md` is the instruction file every assistant reads. Its rules for
file headers, comments and documentation apply to everything you write in the new
repository, including the app's Lua and SQL.

## 3. Choose the slug

The slug is the app's short machine name. It is the folder name, the URL path, and the key
of the app's log, so choose it once.

- It must match `^[a-z][a-z0-9-]{1,30}$`: lowercase letters, digits and hyphens, starting
  with a letter, 2 to 31 characters long.
- Keep it to 15 characters or fewer. The manifest's `advertise = true` announces the app on
  the local network, and that record cannot hold a longer name. The linter reports a
  longer slug as error `PV501`.
- The folder name and the `slug` in `app.toml` must be the same. The node refuses a folder
  whose name and slug differ.

The title is separate and can be up to 40 characters. An app titled "Example Tracker" can
have the slug `tracker`.

## 4. Put the app folder at `apps/<slug>/`

Lay the repository out like the framework's own: one folder per app under `apps/`. Every
command in the assistant guides is written for that layout, and the linter's path argument
works unchanged.

From the repository's root, copy the smallest example app and rename it in one step:

```sh
privatium --data-dir . new tracker --from hello
```

The `--data-dir .` flag tells the program to treat the repository as the data root for this
one command, so the copy lands in `./apps/tracker/`. The `new` command opens no node and
creates nothing but the app folder. Check with `git status` afterwards; only `apps/tracker/`
should be new. Replace `tracker` with your slug.

The copy rewrites what names the app: the `slug` and `title` in the manifest, the
`apps/hello` paths in file headers and READMEs, the `privatium-app-hello` skill name, and
the HTML title. It leaves prose alone. Now finish by hand:

1. In `apps/tracker/app.toml`, set `title`, `description`, `authors`, `version`, `license`
   and `icon`. Icon names come from [the icon guide](icons.md).
2. Rewrite `apps/tracker/README.md` and `apps/tracker/SKILL.md` for your app. The skill
   file is the one an assistant loads when it extends this app later, so keep it current
   as the schema grows.
3. Replace the file headers in the copied `.lua`, `.sql` and `.lsp` files with your
   project's header, as your repository's `AGENTS.md` requires. The copies still carry
   Privatium's.

Add these lines to the repository's `.gitignore`. They matter only if someone runs a node
with `--data-dir .` by mistake, and then they keep private keys and real records out of
git:

```gitignore
# A Privatium data root, if a node is ever started here by mistake
/data/
/cache/
/identity/
/local/
/config.toml
```

## 5. Add the Privatium guides for your assistant

The template gives you four general skills under `skills/`. The framework's own guides are
built into the program, so the version you run is the version you hand to your assistant.
From the repository's root:

```sh
privatium skill export privatium-overview privatium-tier1-lua \
  privatium-security privatium-accessibility --out skills
```

Name the skills you want. Exporting with no names writes every skill and a `README.md`,
and that README would replace the template's. A Tier 2 app exports `privatium-tier2-web`
in place of the Lua guide, and a game adds `privatium-games`.

Commit the exported folders and list them in `SKILLS.md`, one line each, in the same form
as the four already there. After you upgrade Privatium, run the same command again and
commit the difference. The `reference/` folder inside each skill is generated by the
framework; never edit it by hand.

A running node serves the same files at `http://127.0.0.1:8420/skills/bundle.zip` for the
times you do not have the program on the machine you are writing on.

## 6. Pin the framework version

A Tier 1 app has no package manager and no lockfile, so the pin is three plain statements.

1. **`api = 1` in `app.toml`.** This is the contract the app targets. A node that
   implements a lower `api` refuses the app instead of guessing.
2. **A line in the README.** State the release you tested with, for example
   "Tested with Privatium v0.2". Update it when you test a newer one.
3. **A lint job that downloads that exact release.** GitHub Actions can run the linter
   without a Rust toolchain. The example below is the whole workflow; put it at
   `.github/workflows/lint.yml` and change the version and the slug.

   ```yaml
   name: Lint

   on: [push, pull_request]

   env:
     PRIVATIUM_VERSION: v0.2

   jobs:
     lint:
       runs-on: ubuntu-latest
       steps:
         - uses: actions/checkout@v4

         - name: Download the pinned Privatium release
           run: |
             curl -sSL -o privatium-linux.tar.gz \
               "https://github.com/gabrielmongefranco/privatium/releases/download/${PRIVATIUM_VERSION}/privatium-linux.tar.gz"
             tar -xzf privatium-linux.tar.gz

         - name: Lint the app
           run: ./privatium --data-dir "$RUNNER_TEMP/lint-node" lint apps/tracker
   ```

   The archive holds the `privatium` program. The `--data-dir` flag points at an empty
   folder so the lint reads no configuration from anywhere else, which is how the
   framework's own CI runs it. The command exits with code 3 when any finding remains, and
   that fails the job.

A Tier 3 app is a Rust program and pins `privatium-core` in its `Cargo.toml` and
`Cargo.lock` like any other crate. The app contract's section on embedded mode shows the
dependency line.

## 7. Run the app while you work

The node loads apps from the `apps/` folder inside its data directory. Every start prints
that directory on a line beginning `privatium: data in`, and
[backup and restore](backup-and-restore.md) lists where it is on each platform. Link your
checkout into it, using the slug as the link's name:

```sh
ln -s ~/git/privatium-example-tracker/apps/tracker ~/.local/share/privatium/apps/tracker
privatium dev --app tracker
```

The loader follows the link, and the `dev` command prints the app's folder and URL. Save
a file, refresh the browser, and the change is there. On Windows without permission to
create symbolic links, copy the folder into the data directory instead and copy it again
after each change.

Do not start a node with the checkout as its data root. A data root holds your private keys
under `identity/` and your real records under `data/`, and both would then sit inside a git
working tree.

Before each commit, lint from the repository root:

```sh
privatium lint apps/tracker
```

## 8. Write the app

The guides you exported in section 5 hold the rules, the pinned API surface, the common
mistakes and the verification command. Load the tier guide plus the security and
accessibility guides, whether you write the code yourself or an assistant does. The loop
is the same either way: write, run `privatium lint`, fix what it reports, repeat. Do not
call the app finished while the linter reports anything.

Two things belong in the repository from the first commit.

- **A `docs/` folder that says what the app stores.** State the grain of every table, the
  meaning and unit of every column, and which fields hold personal or health information.
  Privatium stores every record as plain text by design, and [the security page](security.md)
  explains what that does and does not protect. Say so plainly in your own README rather
  than implying more.
- **Synthetic sample data in `apps/<slug>/sample/seed.jsonl`.** One event per line, invented
  from start to finish, with no real names, dates of birth or medication records. The
  settings page of a node offers to load it into an app whose log is still empty. A new
  owner can then see the app populated before they type anything.

## 9. Share the app

An app is a folder, so the app contract says distribution is a zip file or a git
repository. There is no app store and no registry, on purpose.

- **A release.** Have your release workflow zip `apps/<slug>/` and attach it. An owner
  extracts it into the `apps/` folder of their data directory and restarts or refreshes the
  node. Name the Privatium version you tested with in the release notes.
- **A clone.** Anyone can clone the repository and link or copy `apps/<slug>/` into place as
  section 7 describes.

Warn owners in your README the way the app contract does. A Tier 1 app's Lua runs on their
node against their data. It is sandboxed from the filesystem, but it is still their node.
Installing an app from a stranger deserves the same thought as running a script someone
emailed.

## 10. A prompt for an AI assistant

Paste the text below into a new session with a coding assistant that can run commands and
read the web. It collects the three answers it needs, then follows this page. Point it at
your own copy of this page if you work from a fork.

```text
I want to start a new Privatium app in its own GitHub repository.

Before you do anything else, ask me for three things and wait for my answers:
1. The app's name. This becomes the title.
2. A one-sentence description of what the app does.
3. The author's name.

From the name, propose a slug of 15 characters or fewer, using only lowercase
letters, digits and hyphens, and confirm it with me before you use it.

Then read the Privatium guide "Your app in its own repository" at
https://github.com/gabrielmongefranco/privatium/blob/main/docs/app-repository.md
and follow its steps in order. Use my answers wherever the guide says title,
description, slug or author. Follow the AGENTS.md of the new repository for
everything you write there, including file headers.

Ask me before you create anything on GitHub, before the first commit, and before
any push. When you finish, show me the folder tree and the output of
`privatium lint`.
```

## Conclusion

You now have an app that lives in its own repository and carries the assistant guides for
the framework version you run. It lints in CI against that same version, and it runs from a
link in your data directory while you work. From here, read the tier guide you exported and
start on the schema, or look at the example apps for a working shape to copy.

## Additional resources

- [Privatium README](../README.md), with the download links and the quick start.
- [Repository template](https://github.com/gabrielmongefranco/repo-template), the starting
  point for the new repository.
- [GitHub command-line tool](https://cli.github.com/), optional, for creating the
  repository from the terminal.
- [App contract](../spec/app-contract.md), the normative definition of an app, its manifest,
  and how apps are published.
- [Command line](../spec/cli.md), for `new`, `dev`, `lint` and `skill export` in full.
- [AI assistant guides](skills.md), how the skills are built and why they are pinned to a
  version.
- [Overview guide](../skills/privatium-overview/SKILL.md), for choosing a tier.
- [Example apps](../apps/README.md), the reference apps and what each one shows.
- [Backup and restore](backup-and-restore.md), which names the data directory on each
  platform.
- [Security](security.md), the threat model, and what plain-text storage means for
  personal data.
- [Icons](icons.md), the icon names `app.toml` accepts.

[Back to the project README](../README.md)

---

Copyright © 2026 Gabriel Mongefranco
