# Contributing

## Layout

```
src/hwdesign/        the library (kicad/: needs pcbnew; mcad/: needs build123d)
tests/               pytest; tests needing pcbnew or build123d skip themselves
skills/<skill>/      SKILL.md plus reference files, loaded on demand
agents/<agent>.md    subagents
.claude-plugin/      plugin manifest and its one-entry marketplace
scripts/gen_api.py   regenerates skills/pcb-as-code/api.md from the source
router/              hwroute, the router (Rust); kicad/negotiate.py builds and runs it
```

The repository root is the plugin, so an installed plugin carries the library with it.

## Rules

- **Keep the library project-agnostic.** No part numbers, board sizes or net names from a
  particular project in `src/`. Projects pass those in.
- **Behaviour changes need a test** where the code can run without KiCad. Move pure logic
  out of pcbnew-dependent modules so it can be tested (as `netlist.py` was).
- **Skills state what happened.** Each rule in a skill says what broke without it. A rule
  without a reason gets deleted by the next person who doesn't understand it.
- **Keep `SKILL.md` short.** Workflow and rules go there; tables, snippets and long
  references go in files next to it, linked from the text.
- **A skill's `description` decides when it is used.** Name the situations and error
  messages that should trigger it.
- **The plugin serves Claude Code and GitHub Copilot CLI.** Both read `.claude-plugin/`,
  `skills/` and `agents/`. An agent that needs a shell lists `Bash` (Claude Code) and
  `execute` (Copilot) in `tools`: Copilot on Windows maps `Bash` to nothing, and the agents
  lost their shell there. Copilot names plugin agents `hw-design:<agent>`.
- **After changing a public signature**, run `python3 scripts/gen_api.py`. CI fails when
  `api.md` is out of date.

## Releases

Semantic versions, in both `pyproject.toml` and `.claude-plugin/plugin.json`. Record changes
in `CHANGELOG.md`. Projects pin a commit through their submodule, so a release never changes
their builds until they update it.

## Checks

```bash
ruff check . && pytest -q && claude plugin validate .
(cd router && cargo clippy --release --all-targets -- -D warnings && cargo test --release)   # if router/ changed
```

CI runs the same checks on every push and pull request.
