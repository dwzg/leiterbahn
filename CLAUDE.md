# CLAUDE.md

This repository is both a Python library (`src/leiterbahn`) and a Claude Code plugin (the
repository root: `skills/`, `agents/`, `.claude-plugin/`). Read `CONTRIBUTING.md` before
changing anything; its rules apply to you.

Before committing: `ruff check . && .venv/bin/pytest -q && claude plugin validate .`, and
`python3 scripts/gen_api.py` if a public signature changed.
