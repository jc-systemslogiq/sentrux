# CLI Advisor

Sentrux can run as a local architecture advisor without sending source code to any hosted service. The CLI scans the working tree, applies `.sentrux/exclude`, computes structural metrics locally, and prints either human text or JSON for agents and repo scripts.

## Commands

```bash
sentrux debt . --limit 10
sentrux debt . --limit 10 --format json
sentrux diagnostics . --limit 10 --format json
sentrux file-detail . path/to/file.ts --format json
sentrux what-if . --remove-file path/to/file.ts --format json
sentrux what-if . --remove-edge from.ts:to.ts --format json
sentrux gate --save .
sentrux gate .
```

## Recommended Agent Loop

1. Run `sentrux debt . --format json` before choosing refactoring targets.
2. Inspect the top target with `sentrux file-detail . <path> --format json`.
3. Use `sentrux what-if` for dependency removals, file moves, or cycle breaks before editing.
4. Make a small refactor with characterization tests.
5. Run `sentrux gate .` to make sure structural metrics did not regress.
6. Run the repo's normal quality gate before completion.

## Interpreting Targets

- `god_file`: high fan-out; usually too much orchestration or too many dependencies.
- `hotspot`: high fan-in; many files depend on it, so changes need stable APIs and focused tests.
- `complex_function`: high cyclomatic complexity; split decisions from IO and state changes.
- `long_function`: large function body; extract cohesive helpers after adding tests.

## What-If Semantics

`sentrux what-if` exits 0 when the simulation runs successfully. It does not fail merely because the proposed change is neutral or worse. Inspect the JSON `improved`, `score_before`, and `score_after` fields before deciding whether to edit.

The `--remove-edge` and `--move-file` flags use `left:right` pairs and are intended for repo-relative Unix-style paths. Paths containing literal `:` are not supported by this first CLI version.

## Repo Setup

Use `.sentrux/exclude` for generated, archived, vendored, or retired paths. Use `.sentrux/rules.toml` for boundaries and baseline-oriented thresholds. Keep `sentrux gate` strict for regressions and keep `sentrux debt` advisory so existing debt remains visible without blocking every build.
