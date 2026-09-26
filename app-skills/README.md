<!--
This file is part of Privatium
Copyright © 2026 Gabriel Mongefranco
Licensed under the GNU Free Documentation License v1.3 or later.
See <https://www.gnu.org/licenses/fdl-1.3.html>. See README for full license information.
-->

# Privatium

## Skills for building apps

[Back to project README](../README.md)

These folders are the guides an AI assistant reads to build an app on Privatium. Each one
carries the rules that make an app correct, the exact API surface of one version of the
program, the mistakes assistants make most often, and the command that proves the result.
They are for anyone writing an app, in any repository. They are not instructions for
working on Privatium itself; those live in the repository's `skills/` folder.

### What is here

| Skill | Load it when |
|---|---|
| `privatium-overview` | Always, and first. It picks the tier and the mode and states the rules that hold everywhere. |
| `privatium-tier1-lua` | The app is Lua with LSP templates: records, lists, forms, reports, trackers. |
| `privatium-tier2-web` | The app serves its own HTML and JavaScript. |
| `privatium-tier3-rust` | The app is a Rust program that uses `privatium-core` as a library. |
| `privatium-games` | The app is a game or uses a game engine. Load it with the Tier 2 skill. |
| `privatium-security` | Every app. |
| `privatium-accessibility` | Every app. |

Each skill's `reference/` folder is generated from the program's own source and the
specification at the version that produced it. Do not edit those files. They are what
makes the skill a pinned reference instead of a paraphrase.

Every reference app also carries a `SKILL.md` of its own, named `privatium-app-<slug>`,
that describes that app's schema and conventions. Give your app one too, so an assistant
that extends it later has the local context.

### Get the version you run

The program embeds these files, so the copy you install matches the version installed on
your computer. There are three ways to get them. Run the commands from the root of the
repository that will hold the app.

1. **From the program.** Name the skills you want, and they land under `skills/`:

   ```sh
   privatium skill export privatium-overview privatium-tier1-lua \
     privatium-security privatium-accessibility --out skills
   ```

   `privatium skill export` with no names writes every skill and this README. If your
   repository already has a `skills/README.md`, export by name so it is not replaced.

2. **From a node running on this computer.** The node serves the whole set as one zip
   file. Extract it into your `skills/` folder:

   ```sh
   curl -sSL -o privatium-skills.zip http://127.0.0.1:8420/skills/bundle.zip
   unzip -o privatium-skills.zip -d skills
   ```

   This works only from the computer the node runs on. Over plain HTTP, a node answers
   other machines with its bootstrap pages alone.

3. **From a checkout.** Copy the folders in `app-skills/` at the git tag that matches
   your version.

`privatium --version` prints the version, and `privatium skill list` prints every skill
this build ships with its one-line description. After you upgrade the program, run the
same export again and commit the difference.

### Use them

In a repository built from the author's template, list the exported skills in
`SKILLS.md`, one line each, beside the general skills already there. Load the overview
first, then the tier skill, then the security and accessibility skills. Review any file
you install before an assistant reads it, the same as you would any other download.

### Verify

```sh
privatium lint apps/<slug>
```

Every skill ends with this command. Do not call an app finished while it reports
anything.

### Conclusion

Install the set that matches your program, load the overview and your tier, and let the
linter close the loop. When the program changes, export again.

### Additional resources

- [Your app in its own repository](../docs/app-repository.md), the setup guide for an app
  in a repository of its own.
- [AI assistant guides](../docs/skills.md), how these skills are built, distributed and
  kept in step with the specification.
- [Command line](../spec/cli.md), for `skill list`, `skill export` and `lint` in full.
- [App contract](../spec/app-contract.md), what an app is.
- [Example apps](../apps/README.md), the reference apps and their own skill files.

[Back to project README](../README.md)
