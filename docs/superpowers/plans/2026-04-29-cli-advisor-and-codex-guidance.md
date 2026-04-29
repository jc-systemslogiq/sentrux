# CLI Advisor And Codex Guidance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a CLI-first architecture advisor for our customized Sentrux fork and install Codex guidance so agents use it consistently inside any repo.

**Architecture:** Keep Sentrux as a local CLI tool. Move advisory report construction into `sentrux-core` so `debt`, `diagnostics`, and `file-detail` share one typed advisor model. Keep `what-if` on its existing typed simulation result schema and make it JSON-renderable from the CLI. Keep `sentrux-bin` focused on argument parsing, scanning, and rendering text or JSON. Add a global Codex skill that tells agents when to run Sentrux, how to interpret the output, and how to use architecture signals without outsourcing source code to a third party.

**Tech Stack:** Rust, Clap, Serde JSON, existing Sentrux scanner and metrics modules, global Codex user skills under `/Users/james/.codex/skills`, global Codex guidance in `/Users/james/.codex/AGENTS.md`.

---

## Scope

This plan intentionally avoids GUI work and unreleased Pro plugin behavior. It exposes the already-useful analysis as CLI and JSON reports, then teaches Codex how to use the customized local CLI.

Do not push the fork during implementation. Local commits are useful for review checkpoints; publishing waits for human review.

## File Structure

- Create `sentrux-core/src/metrics/advisor.rs`: typed advisory reports, ranking logic, file-detail report construction, and JSON-serializable DTOs.
- Modify `sentrux-core/src/metrics/mod.rs`: export the new advisor module.
- Modify `sentrux-core/src/metrics/whatif/mod.rs`: make existing what-if result DTOs JSON-serializable.
- Modify `sentrux-bin/Cargo.toml`: add a direct `serde_json` dependency for CLI JSON rendering.
- Modify `Cargo.lock`: record the binary crate's direct dependency metadata after `cargo check`.
- Modify `sentrux-bin/src/main_impl.rs`: add `--format json` to `debt`, add `diagnostics`, `file-detail`, and `what-if` subcommands, and delegate report construction to `metrics::advisor`.
- Create `docs/cli-advisor.md`: CLI usage, JSON schema examples, and repo integration workflow.
- Modify `README.md`: add the CLI advisor commands to the run section.
- Create `/Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md`: user-level Codex skill for using customized Sentrux.
- Modify `/Users/james/.codex/AGENTS.md`: short global trigger rule to use the skill for architecture/refactoring/debt questions.

## CLI Surface

```bash
sentrux debt . --limit 10
sentrux debt . --limit 10 --format json
sentrux diagnostics . --limit 10 --format json
sentrux file-detail . frontend/src/features/diagrams/runtime/useDiagramRuntimeSession.ts --format json
sentrux what-if . --remove-file frontend/src/features/diagrams/runtime/useDiagramRuntimeSession.ts --format json
sentrux what-if . --remove-edge frontend/src/stores/MeasurementStore.ts:frontend/src/stores/ProjectionStore.ts --format json
sentrux what-if . --move-file old/path.ts:new/path.ts --format json
sentrux what-if . --break-cycle a.ts,b.ts,c.ts --format json
```

The default output remains human text. JSON is stable enough for agents and repo scripts.

### Task 1: Add Core Advisor Report Types

**Files:**
- Create: `sentrux-core/src/metrics/advisor.rs`
- Modify: `sentrux-core/src/metrics/mod.rs`
- Test: `sentrux-core/src/metrics/advisor.rs`

- [ ] **Step 1: Write the failing advisor ranking test**

Create `sentrux-core/src/metrics/advisor.rs` with the test below. Before running the test, insert `pub mod advisor;` into `sentrux-core/src/metrics/mod.rs`; otherwise the new file will not be compiled by the crate test target.

```rust
//! Architecture advisor reports for CLI and agent consumption.

use super::{FileMetric, FuncMetric};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_complex_god_file_above_plain_fan_out() {
        let report = build_advice_report_from_parts(AdviceParts {
            quality_signal: 0.42,
            coupling_score: 0.31,
            cycle_count: 1,
            max_depth: 8,
            god_files: vec![
                FileMetric { path: "src/plain.rs".into(), value: 18 },
                FileMetric { path: "src/risky.rs".into(), value: 16 },
            ],
            hotspot_files: vec![],
            complex_functions: vec![
                FuncMetric { file: "src/risky.rs".into(), func: "run".into(), value: 91 },
            ],
            long_functions: vec![],
            cycles: vec![vec!["src/a.rs".into(), "src/b.rs".into()]],
            limit: 10,
        });

        assert_eq!(report.targets[0].path, "src/risky.rs");
        assert_eq!(report.targets[0].category, AdviceCategory::ComplexFunction);
        assert_eq!(report.targets[0].symbol.as_deref(), Some("run"));
        assert!(report.targets.iter().any(|target| {
            target.path == "src/risky.rs" && target.category == AdviceCategory::GodFile
        }));
        assert!(report.targets[0].priority > report.targets[1].priority);
        assert_eq!(report.summary.quality_signal, 4200);
    }
}
```

Run:

```bash
cargo test -p sentrux-core advisor::tests::ranks_complex_god_file_above_plain_fan_out
```

Expected: FAIL because `build_advice_report_from_parts`, `AdviceParts`, `AdviceCategory`, and report types do not exist.

- [ ] **Step 2: Implement the typed advisor model**

Replace `sentrux-core/src/metrics/advisor.rs` with:

```rust
//! Architecture advisor reports for CLI and agent consumption.

use super::arch::ArchReport;
use super::{FileMetric, FuncMetric, HealthReport};
use crate::core::snapshot::{flatten_files_ref, Snapshot};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AdviceCategory {
    GodFile,
    Hotspot,
    Cycle,
    ComplexFunction,
    LongFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdviceSummary {
    pub quality_signal: u32,
    pub coupling_score: f64,
    pub cycle_count: usize,
    pub god_file_count: usize,
    pub hotspot_count: usize,
    pub complex_function_count: usize,
    pub long_function_count: usize,
    pub max_depth: u32,
    pub main_sequence_distance: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdviceTarget {
    pub category: AdviceCategory,
    pub path: String,
    pub symbol: Option<String>,
    pub priority: f64,
    pub evidence: BTreeMap<String, serde_json::Value>,
    pub suggested_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdviceReport {
    pub summary: AdviceSummary,
    pub targets: Vec<AdviceTarget>,
    pub cycles: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct AdviceParts {
    pub quality_signal: f64,
    pub coupling_score: f64,
    pub cycle_count: usize,
    pub max_depth: u32,
    pub god_files: Vec<FileMetric>,
    pub hotspot_files: Vec<FileMetric>,
    pub complex_functions: Vec<FuncMetric>,
    pub long_functions: Vec<FuncMetric>,
    pub cycles: Vec<Vec<String>>,
    pub limit: usize,
}

pub fn build_advice_report(
    health: &HealthReport,
    arch: &ArchReport,
    limit: usize,
) -> AdviceReport {
    build_advice_report_from_parts(AdviceParts {
        quality_signal: health.quality_signal,
        coupling_score: health.coupling_score,
        cycle_count: health.circular_dep_count,
        max_depth: health.max_depth,
        god_files: health.god_files.clone(),
        hotspot_files: health.hotspot_files.clone(),
        complex_functions: health.complex_functions.clone(),
        long_functions: health.long_functions.clone(),
        cycles: health.circular_dep_files.clone(),
        limit,
    })
    .with_main_sequence_distance(if arch.distance_metrics.is_empty() {
        None
    } else {
        Some(round2(arch.avg_distance))
    })
}

pub fn build_advice_report_from_parts(parts: AdviceParts) -> AdviceReport {
    let limit = parts.limit.max(1);
    let mut index: HashMap<String, AdviceTarget> = HashMap::new();
    let complex_by_file = max_func_values(&parts.complex_functions);
    let long_by_file = max_func_values(&parts.long_functions);

    for metric in &parts.god_files {
        let mut evidence = BTreeMap::new();
        evidence.insert("fan_out".into(), metric.value.into());
        if let Some(cc) = complex_by_file.get(&metric.path) {
            evidence.insert("max_cyclomatic_complexity".into(), (*cc).into());
        }
        if let Some(lines) = long_by_file.get(&metric.path) {
            evidence.insert("max_function_lines".into(), (*lines).into());
        }
        upsert_target(&mut index, AdviceTarget {
            category: AdviceCategory::GodFile,
            path: metric.path.clone(),
            symbol: None,
            priority: metric.value as f64 + complex_by_file.get(&metric.path).copied().unwrap_or(0) as f64 * 0.25,
            evidence,
            suggested_actions: vec![
                "Split orchestration from leaf behavior".into(),
                "Move stable interfaces into a smaller module".into(),
                "Add characterization tests before extracting code".into(),
            ],
        });
    }

    for metric in &parts.hotspot_files {
        let mut evidence = BTreeMap::new();
        evidence.insert("fan_in".into(), metric.value.into());
        upsert_target(&mut index, AdviceTarget {
            category: AdviceCategory::Hotspot,
            path: metric.path.clone(),
            symbol: None,
            priority: metric.value as f64 * 1.2,
            evidence,
            suggested_actions: vec![
                "Stabilize public API before changing internals".into(),
                "Prefer adapter seams over direct consumers".into(),
            ],
        });
    }

    for metric in &parts.complex_functions {
        let mut evidence = BTreeMap::new();
        evidence.insert("cyclomatic_complexity".into(), metric.value.into());
        upsert_target(&mut index, AdviceTarget {
            category: AdviceCategory::ComplexFunction,
            path: metric.file.clone(),
            symbol: Some(metric.func.clone()),
            priority: metric.value as f64,
            evidence,
            suggested_actions: vec![
                "Extract named decision branches".into(),
                "Separate IO, validation, and transformation logic".into(),
            ],
        });
    }

    for metric in &parts.long_functions {
        let mut evidence = BTreeMap::new();
        evidence.insert("function_lines".into(), metric.value.into());
        upsert_target(&mut index, AdviceTarget {
            category: AdviceCategory::LongFunction,
            path: metric.file.clone(),
            symbol: Some(metric.func.clone()),
            priority: metric.value as f64 / 10.0,
            evidence,
            suggested_actions: vec![
                "Extract cohesive helper functions".into(),
                "Move state setup away from command logic".into(),
            ],
        });
    }

    let mut targets: Vec<AdviceTarget> = index.into_values().collect();
    targets.sort_by(|a, b| {
        b.priority
            .partial_cmp(&a.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.category.cmp(&b.category))
    });
    targets.truncate(limit);

    AdviceReport {
        summary: AdviceSummary {
            quality_signal: score(parts.quality_signal),
            coupling_score: round2(parts.coupling_score),
            cycle_count: parts.cycle_count,
            god_file_count: parts.god_files.len(),
            hotspot_count: parts.hotspot_files.len(),
            complex_function_count: parts.complex_functions.len(),
            long_function_count: parts.long_functions.len(),
            max_depth: parts.max_depth,
            main_sequence_distance: None,
        },
        targets,
        cycles: parts.cycles.into_iter().take(limit).collect(),
    }
}

impl AdviceReport {
    fn with_main_sequence_distance(mut self, value: Option<f64>) -> Self {
        self.summary.main_sequence_distance = value;
        self
    }
}

fn upsert_target(index: &mut HashMap<String, AdviceTarget>, next: AdviceTarget) {
    let key = format!("{}::{}", next.path, next.symbol.clone().unwrap_or_default());
    match index.get_mut(&key) {
        Some(existing) if next.priority > existing.priority => *existing = next,
        Some(_) => {}
        None => {
            index.insert(key, next);
        }
    }
}

fn max_func_values(metrics: &[FuncMetric]) -> HashMap<String, u32> {
    let mut result = HashMap::new();
    for metric in metrics {
        let entry = result.entry(metric.file.clone()).or_insert(0);
        *entry = (*entry).max(metric.value);
    }
    result
}

fn score(value: f64) -> u32 {
    (value * 10000.0).round() as u32
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
```

- [ ] **Step 3: Confirm the advisor module export**

Verify `sentrux-core/src/metrics/mod.rs` still contains its existing module declarations and that this line is present exactly once near the top-level metric modules, before `pub mod arch;`:

```rust
pub mod advisor;
```

Do not duplicate the declaration or replace the module list; keep existing declarations such as `cross_validation`, `dsm`, `root_causes`, `stability`, `testgap`, `types`, `whatif`, `pub use types::*`, and `pub use evo as evolution`.

Run:

```bash
cargo test -p sentrux-core advisor::tests::ranks_complex_god_file_above_plain_fan_out
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add sentrux-core/src/metrics/advisor.rs sentrux-core/src/metrics/mod.rs
git commit -m "feat: add architecture advisor report model"
```

### Task 2: Add JSON Output To `sentrux debt`

**Files:**
- Modify: `Cargo.lock`
- Modify: `sentrux-bin/Cargo.toml`
- Modify: `sentrux-bin/src/main_impl.rs`
- Test: `sentrux-bin/src/main_impl.rs`

- [ ] **Step 1: Write failing CLI parser tests**

Add this test module near the end of `sentrux-bin/src/main_impl.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_debt_json_format() {
        let cli = Cli::try_parse_from(["sentrux", "debt", ".", "--format", "json"]).unwrap();
        match cli.command {
            Some(Command::Debt { format, .. }) => assert_eq!(format, OutputFormat::Json),
            _ => panic!("expected debt command"),
        }
    }
}
```

Run:

```bash
cargo test -p sentrux --lib parses_debt_json_format
```

Expected: FAIL because `OutputFormat` does not exist and `Command::Debt` has no `format` field.

- [ ] **Step 2: Add output format parsing**

Modify `sentrux-bin/src/main_impl.rs` near the CLI definitions:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}
```

Update the `Debt` command:

```rust
Debt {
    /// Directory to inspect
    #[arg(default_value = ".")]
    path: String,

    /// Maximum rows to print per section
    #[arg(long, default_value_t = 10)]
    limit: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
},
```

Update the match arm:

```rust
Some(Command::Debt { path, limit, format }) => {
    std::process::exit(run_debt(&path, limit, format));
}
```

Change the signature:

```rust
fn run_debt(path: &str, limit: usize, format: OutputFormat) -> i32 {
```

Run:

```bash
cargo test -p sentrux --lib parses_debt_json_format
```

Expected: PASS.

- [ ] **Step 3: Add direct JSON dependency**

Add `serde_json` to `sentrux-bin/Cargo.toml` because the binary crate renders JSON directly:

```toml
serde_json = "1"
```

Run:

```bash
cargo check -p sentrux --lib
```

Expected: PASS.

This command may update `Cargo.lock` to record the `sentrux` package's direct dependency metadata. Include that lockfile change in the Task 2 commit.

- [ ] **Step 4: Add JSON rendering**

Replace the end of `run_debt`:

```rust
let health = metrics::compute_health(&result.snapshot);
let arch_report = metrics::arch::compute_arch(&result.snapshot);
let report = metrics::advisor::build_advice_report(&health, &arch_report, limit.max(1));

match format {
    OutputFormat::Text => print_debt_report(&health, &arch_report, limit.max(1)),
    OutputFormat::Json => match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("Failed to render JSON: {e}");
            return 1;
        }
    },
}
0
```

Keep `print_debt_report` for human output.

Run:

```bash
cargo test -p sentrux --lib parses_debt_json_format
cargo run -p sentrux -- debt . --limit 3 --format json
```

Expected: tests PASS and the command prints JSON with `summary`, `targets`, and `cycles`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.lock sentrux-bin/Cargo.toml sentrux-bin/src/main_impl.rs
git commit -m "feat: add json output for debt report"
```

### Task 3: Add `sentrux diagnostics`

**Files:**
- Modify: `sentrux-bin/src/main_impl.rs`
- Modify: `README.md`
- Test: `sentrux-bin/src/main_impl.rs`

- [ ] **Step 1: Write failing parser test**

Extend the existing test module:

```rust
#[test]
fn parses_diagnostics_json_format() {
    let cli = Cli::try_parse_from(["sentrux", "diagnostics", ".", "--limit", "5", "--format", "json"]).unwrap();
    match cli.command {
        Some(Command::Diagnostics { path, limit, format }) => {
            assert_eq!(path, ".");
            assert_eq!(limit, 5);
            assert_eq!(format, OutputFormat::Json);
        }
        _ => panic!("expected diagnostics command"),
    }
}
```

Run:

```bash
cargo test -p sentrux --lib parses_diagnostics_json_format
```

Expected: FAIL because `Command::Diagnostics` does not exist.

- [ ] **Step 2: Add the command and runner**

Add the enum variant:

```rust
/// Print root-cause-organized architecture diagnostics
Diagnostics {
    /// Directory to inspect
    #[arg(default_value = ".")]
    path: String,

    /// Maximum rows to print per section
    #[arg(long, default_value_t = 10)]
    limit: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
},
```

Add the match arm:

```rust
Some(Command::Diagnostics { path, limit, format }) => {
    std::process::exit(run_diagnostics(&path, limit, format));
}
```

Add the runner after `run_debt`:

```rust
fn run_diagnostics(path: &str, limit: usize, format: OutputFormat) -> i32 {
    let root = std::path::Path::new(path);
    if !root.is_dir() {
        eprintln!("Error: not a directory: {path}");
        return 1;
    }

    eprintln!("Scanning {path}...");
    let result = match analysis::scanner::scan_directory(path, None, None, &cli_scan_limits(), None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Scan failed: {e}");
            return 1;
        }
    };

    let health = metrics::compute_health(&result.snapshot);
    let arch_report = metrics::arch::compute_arch(&result.snapshot);
    let report = metrics::advisor::build_advice_report(&health, &arch_report, limit.max(1));

    match format {
        OutputFormat::Text => print_diagnostics_report(&report),
        OutputFormat::Json => match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to render JSON: {e}");
                return 1;
            }
        },
    }
    0
}

fn print_diagnostics_report(report: &metrics::advisor::AdviceReport) {
    println!("sentrux diagnostics - root-cause refactoring queue\n");
    println!("Quality: {}", report.summary.quality_signal);
    println!("Cycles:  {}", report.summary.cycle_count);
    println!("Targets:");
    for target in &report.targets {
        match &target.symbol {
            Some(symbol) => println!("  {:?}: {}::{} priority={:.2}", target.category, target.path, symbol, target.priority),
            None => println!("  {:?}: {} priority={:.2}", target.category, target.path, target.priority),
        }
    }
}
```

Run:

```bash
cargo test -p sentrux --lib parses_diagnostics_json_format
cargo run -p sentrux -- diagnostics . --limit 5 --format json
```

Expected: tests PASS and JSON diagnostics print.

- [ ] **Step 3: Update README command list**

In `README.md`, replace the existing run command block with:

```bash
sentrux                    # open the GUI - live treemap of your project
sentrux /path/to/project   # open GUI scanning a specific directory
sentrux check .            # check rules (CI-friendly, exits 0 or 1)
sentrux gate --save .      # save baseline before agent session
sentrux gate .             # compare after - catches degradation
sentrux debt .             # print advisory refactoring targets
sentrux diagnostics . --format json  # machine-readable agent diagnostics
```

Run:

```bash
cargo test -p sentrux --lib parses_diagnostics_json_format
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add sentrux-bin/src/main_impl.rs README.md
git commit -m "feat: add diagnostics cli report"
```

### Task 4: Add `sentrux file-detail`

**Files:**
- Modify: `sentrux-core/src/metrics/advisor.rs`
- Modify: `sentrux-bin/src/main_impl.rs`
- Test: `sentrux-core/src/metrics/advisor.rs`, `sentrux-bin/src/main_impl.rs`

- [ ] **Step 1: Write failing core test**

Add inside `sentrux-core/src/metrics/advisor.rs` test module:

```rust
#[test]
fn file_detail_reports_missing_file() {
    let snapshot = crate::metrics::test_helpers::snap_with_edges(vec![], vec![]);
    let detail = build_file_detail_report(&snapshot, &[], None, "src/missing.rs");
    assert_eq!(detail.path, "src/missing.rs");
    assert!(!detail.found);
    assert!(detail.imports.is_empty());
    assert!(detail.imported_by.is_empty());
}
```

Run:

```bash
cargo test -p sentrux-core advisor::tests::file_detail_reports_missing_file
```

Expected: FAIL because `build_file_detail_report` does not exist.

- [ ] **Step 2: Implement file detail DTO**

Add to `sentrux-core/src/metrics/advisor.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDetail {
    pub name: String,
    pub cyclomatic_complexity: Option<u32>,
    pub lines: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDetailReport {
    pub path: String,
    pub found: bool,
    pub language: Option<String>,
    pub lines: Option<u32>,
    pub functions: Vec<FunctionDetail>,
    pub imports: Vec<String>,
    pub imported_by: Vec<String>,
    pub blast_radius: Option<u32>,
}

pub fn build_file_detail_report(
    snapshot: &Snapshot,
    import_graph: &[crate::core::types::ImportEdge],
    arch: Option<&ArchReport>,
    path: &str,
) -> FileDetailReport {
    let files = flatten_files_ref(&snapshot.root);
    let file = files.iter().find(|node| node.path == path);
    let functions = file
        .and_then(|node| node.sa.as_ref())
        .and_then(|analysis| analysis.functions.as_ref())
        .map(|functions| {
            let mut details: Vec<FunctionDetail> = functions
                .iter()
                .map(|function| FunctionDetail {
                    name: function.n.clone(),
                    cyclomatic_complexity: function.cc,
                    lines: function.ln,
                })
                .collect();
            details.sort_by(|a, b| {
                b.cyclomatic_complexity
                    .unwrap_or(0)
                    .cmp(&a.cyclomatic_complexity.unwrap_or(0))
                    .then_with(|| a.name.cmp(&b.name))
            });
            details
        })
        .unwrap_or_default();

    let mut imports: Vec<String> = import_graph
        .iter()
        .filter(|edge| edge.from_file == path)
        .map(|edge| edge.to_file.clone())
        .collect();
    imports.sort();

    let mut imported_by: Vec<String> = import_graph
        .iter()
        .filter(|edge| edge.to_file == path)
        .map(|edge| edge.from_file.clone())
        .collect();
    imported_by.sort();

    FileDetailReport {
        path: path.into(),
        found: file.is_some(),
        language: file.map(|node| node.lang.clone()),
        lines: file.map(|node| node.lines),
        functions,
        imports,
        imported_by,
        blast_radius: arch.and_then(|report| report.blast_radius.get(path).copied()),
    }
}
```

Run:

```bash
cargo test -p sentrux-core advisor::tests::file_detail_reports_missing_file
```

Expected: PASS.

- [ ] **Step 3: Add CLI parser test**

Add to `sentrux-bin/src/main_impl.rs` tests:

```rust
#[test]
fn parses_file_detail_json_format() {
    let cli = Cli::try_parse_from([
        "sentrux",
        "file-detail",
        ".",
        "src/main.rs",
        "--format",
        "json",
    ]).unwrap();
    match cli.command {
        Some(Command::FileDetail { path, file, format }) => {
            assert_eq!(path, ".");
            assert_eq!(file, "src/main.rs");
            assert_eq!(format, OutputFormat::Json);
        }
        _ => panic!("expected file-detail command"),
    }
}
```

Run:

```bash
cargo test -p sentrux --lib parses_file_detail_json_format
```

Expected: FAIL because the command does not exist.

- [ ] **Step 4: Add command and runner**

Add enum variant:

```rust
/// Print focused metrics for one file
FileDetail {
    /// Directory to inspect
    #[arg(default_value = ".")]
    path: String,

    /// Repo-relative file path
    file: String,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
},
```

Add match arm:

```rust
Some(Command::FileDetail { path, file, format }) => {
    std::process::exit(run_file_detail(&path, &file, format));
}
```

Add runner:

```rust
fn run_file_detail(path: &str, file: &str, format: OutputFormat) -> i32 {
    let root = std::path::Path::new(path);
    if !root.is_dir() {
        eprintln!("Error: not a directory: {path}");
        return 1;
    }

    eprintln!("Scanning {path}...");
    let result = match analysis::scanner::scan_directory(path, None, None, &cli_scan_limits(), None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Scan failed: {e}");
            return 1;
        }
    };
    let arch_report = metrics::arch::compute_arch(&result.snapshot);
    let report = metrics::advisor::build_file_detail_report(
        &result.snapshot,
        &result.snapshot.import_graph,
        Some(&arch_report),
        file,
    );

    match format {
        OutputFormat::Text => print_file_detail_report(&report),
        OutputFormat::Json => match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to render JSON: {e}");
                return 1;
            }
        },
    }
    if report.found { 0 } else { 1 }
}

fn print_file_detail_report(report: &metrics::advisor::FileDetailReport) {
    println!("sentrux file-detail\n");
    println!("Path:  {}", report.path);
    println!("Found: {}", report.found);
    if let Some(language) = &report.language {
        println!("Lang:  {language}");
    }
    if let Some(lines) = report.lines {
        println!("Lines: {lines}");
    }
    println!("Imports:     {}", report.imports.len());
    println!("Imported by: {}", report.imported_by.len());
    if let Some(radius) = report.blast_radius {
        println!("Blast:       {radius}");
    }
    for function in report.functions.iter().take(20) {
        println!(
            "  {} cc={}",
            function.name,
            function.cyclomatic_complexity.unwrap_or(0)
        );
    }
}
```

Run:

```bash
cargo test -p sentrux-core advisor::tests::file_detail_reports_missing_file
cargo test -p sentrux --lib parses_file_detail_json_format
cargo run -p sentrux -- file-detail . sentrux-bin/src/main_impl.rs --format json
```

Expected: tests PASS and file detail JSON prints.

- [ ] **Step 5: Commit**

```bash
git add sentrux-core/src/metrics/advisor.rs sentrux-bin/src/main_impl.rs
git commit -m "feat: add file detail cli report"
```

### Task 5: Add `sentrux what-if`

**Files:**
- Modify: `sentrux-core/src/metrics/whatif/mod.rs`
- Modify: `sentrux-bin/src/main_impl.rs`
- Test: `sentrux-bin/src/main_impl.rs`, `sentrux-core/src/metrics/whatif/tests.rs`

- [ ] **Step 1: Write failing parse tests**

Add to `sentrux-bin/src/main_impl.rs` tests:

```rust
#[test]
fn parses_what_if_remove_edge_action() {
    let action = parse_what_if_action(
        Some("a.rs:b.rs".into()),
        None,
        None,
        None,
    ).unwrap();
    match action {
        metrics::whatif::WhatIfAction::RemoveEdge { from, to } => {
            assert_eq!(from, "a.rs");
            assert_eq!(to, "b.rs");
        }
        _ => panic!("expected remove edge"),
    }
}

#[test]
fn rejects_multiple_what_if_actions() {
    let error = parse_what_if_action(
        Some("a.rs:b.rs".into()),
        Some("a.rs".into()),
        None,
        None,
    ).unwrap_err();
    assert!(error.contains("exactly one"));
}
```

Run:

```bash
cargo test -p sentrux --lib parses_what_if_remove_edge_action
cargo test -p sentrux --lib rejects_multiple_what_if_actions
```

Expected: FAIL because `parse_what_if_action` does not exist.

- [ ] **Step 2: Add parser helper**

Add to `sentrux-bin/src/main_impl.rs` near the debt helpers:

```rust
fn parse_what_if_action(
    remove_edge: Option<String>,
    remove_file: Option<String>,
    move_file: Option<String>,
    break_cycle: Option<String>,
) -> Result<metrics::whatif::WhatIfAction, String> {
    let count = remove_edge.is_some() as usize
        + remove_file.is_some() as usize
        + move_file.is_some() as usize
        + break_cycle.is_some() as usize;
    if count != 1 {
        return Err("provide exactly one what-if action".into());
    }

    if let Some(edge) = remove_edge {
        let (from, to) = split_pair(&edge, "remove-edge")?;
        return Ok(metrics::whatif::WhatIfAction::RemoveEdge { from, to });
    }
    if let Some(path) = remove_file {
        return Ok(metrics::whatif::WhatIfAction::RemoveFile { path });
    }
    if let Some(pair) = move_file {
        let (old_path, new_path) = split_pair(&pair, "move-file")?;
        return Ok(metrics::whatif::WhatIfAction::MoveFile { old_path, new_path });
    }
    if let Some(files) = break_cycle {
        let values: Vec<String> = files
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from)
            .collect();
        if values.len() < 2 {
            return Err("break-cycle requires at least two comma-separated files".into());
        }
        return Ok(metrics::whatif::WhatIfAction::BreakCycle { files: values });
    }
    Err("provide exactly one what-if action".into())
}

fn split_pair(value: &str, flag: &str) -> Result<(String, String), String> {
    let Some((left, right)) = value.split_once(':') else {
        return Err(format!("{flag} expects left:right"));
    };
    if left.trim().is_empty() || right.trim().is_empty() {
        return Err(format!("{flag} expects non-empty left:right"));
    }
    Ok((left.trim().into(), right.trim().into()))
}
```

Run:

```bash
cargo test -p sentrux --lib parses_what_if_remove_edge_action
cargo test -p sentrux --lib rejects_multiple_what_if_actions
```

Expected: PASS.

- [ ] **Step 3: Make what-if results serializable**

In `sentrux-core/src/metrics/whatif/mod.rs`, add unconditional `serde::Serialize` derives to the existing result DTOs:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct WhatIfResult {
    // existing fields stay unchanged
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LevelChange {
    // existing fields stay unchanged
}
```

Do not change the existing what-if schema in this step.

Run:

```bash
cargo test -p sentrux-core whatif
```

Expected: PASS.

- [ ] **Step 4: Add command and runner**

Add enum variant:

```rust
/// Simulate an architecture graph change without editing files
WhatIf {
    /// Directory to inspect
    #[arg(default_value = ".")]
    path: String,

    /// Remove an import edge, formatted as from:to
    #[arg(long)]
    remove_edge: Option<String>,

    /// Remove one file from the graph
    #[arg(long)]
    remove_file: Option<String>,

    /// Move one file, formatted as old:new
    #[arg(long)]
    move_file: Option<String>,

    /// Break a cycle, formatted as comma-separated files
    #[arg(long)]
    break_cycle: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
},
```

Add match arm:

```rust
Some(Command::WhatIf { path, remove_edge, remove_file, move_file, break_cycle, format }) => {
    std::process::exit(run_what_if(&path, remove_edge, remove_file, move_file, break_cycle, format));
}
```

Add runner:

```rust
fn run_what_if(
    path: &str,
    remove_edge: Option<String>,
    remove_file: Option<String>,
    move_file: Option<String>,
    break_cycle: Option<String>,
    format: OutputFormat,
) -> i32 {
    let action = match parse_what_if_action(remove_edge, remove_file, move_file, break_cycle) {
        Ok(action) => action,
        Err(e) => {
            eprintln!("Invalid what-if action: {e}");
            return 2;
        }
    };
    let root = std::path::Path::new(path);
    if !root.is_dir() {
        eprintln!("Error: not a directory: {path}");
        return 1;
    }

    eprintln!("Scanning {path}...");
    let result = match analysis::scanner::scan_directory(path, None, None, &cli_scan_limits(), None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Scan failed: {e}");
            return 1;
        }
    };
    let report = metrics::whatif::simulate(
        &result.snapshot.import_graph,
        &result.snapshot.entry_points,
        &action,
    );

    match format {
        OutputFormat::Text => print_what_if_report(&report),
        OutputFormat::Json => match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to render JSON: {e}");
                return 1;
            }
        },
    }
    0
}

fn print_what_if_report(report: &metrics::whatif::WhatIfResult) {
    println!("sentrux what-if\n");
    println!("Action: {}", report.action_description);
    println!("Score:  {} -> {}", report.score_before, report.score_after);
    println!("Depth:  {} -> {}", report.max_level_before, report.max_level_after);
    println!("Blast:  {} -> {}", report.max_blast_before, report.max_blast_after);
    println!("Result: {}", if report.improved { "improves architecture" } else { "does not improve architecture" });
}
```

`run_what_if` exits 0 when the simulation ran successfully, regardless of whether the proposed change improves the score. CI or agents that need to gate on improvement must read the JSON `improved` field.

Run:

```bash
cargo test -p sentrux --lib parses_what_if_remove_edge_action
cargo test -p sentrux --lib rejects_multiple_what_if_actions
cargo test -p sentrux-core whatif
cargo run -p sentrux -- what-if . --remove-file sentrux-bin/src/main_impl.rs --format json
```

Expected: tests PASS and the command prints a JSON what-if result.

- [ ] **Step 5: Commit**

```bash
git add sentrux-bin/src/main_impl.rs sentrux-core/src/metrics/whatif/mod.rs
git commit -m "feat: add what-if cli simulation"
```

### Task 6: Add CLI Advisor Documentation

**Files:**
- Create: `docs/cli-advisor.md`
- Modify: `README.md`

- [ ] **Step 1: Create CLI advisor docs**

Create `docs/cli-advisor.md`:

````markdown
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
````

Run:

```bash
test -s docs/cli-advisor.md
```

Expected: exit 0.

- [ ] **Step 2: Link docs from README**

Add below the command block in `README.md`:

```markdown
For CLI-first agent workflows, see [CLI Advisor](docs/cli-advisor.md).
```

Run:

```bash
rg -n "CLI Advisor|sentrux diagnostics|sentrux debt" README.md docs/cli-advisor.md
```

Expected: at least one match in each file.

- [ ] **Step 3: Commit**

```bash
git add README.md docs/cli-advisor.md
git commit -m "docs: document cli advisor workflow"
```

### Task 7: Add Global Codex Sentrux Skill

**Files:**
- Create: `/Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md`
- Modify: `/Users/james/.codex/AGENTS.md`

- [ ] **Step 1: Create the global user skill**

Before editing global user configuration, create the skill directory and back up any existing skill file:

```bash
mkdir -p /Users/james/.codex/skills/sentrux-architecture-advisor
test ! -f /Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md || cp -p /Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md "/Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md.bak.$(date +%Y%m%d%H%M%S)"
```

Create or replace `/Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md`:

````markdown
---
name: sentrux-architecture-advisor
description: Use when improving architecture, maintainability, refactoring targets, code smell, dependency cycles, god files, hotspots, blast radius, or structural regressions.
---

# Sentrux Architecture Advisor

Use the local customized `sentrux` CLI to gather architecture signal before proposing refactors or making maintainability changes. Sentrux scans locally and should not require uploading source code.

## When To Run

- Before selecting refactoring targets.
- When the user asks about architecture debt, maintainability, code smell, god files, hotspots, cycles, or blast radius.
- After a refactor, before claiming architecture improved.
- When a repo contains `.sentrux/rules.toml`, `.sentrux/exclude`, or package scripts wrapping Sentrux.

## Workflow

1. Check whether Sentrux is installed:

```bash
command -v sentrux
```

2. Run the advisory queue:

```bash
sentrux debt . --limit 10 --format json
```

3. Inspect the top file before editing:

```bash
sentrux file-detail . <repo-relative-path> --format json
```

4. For dependency removals, moves, and cycle breaks, simulate first:

```bash
sentrux what-if . --remove-edge from.ts:to.ts --format json
sentrux what-if . --remove-file path/to/file.ts --format json
sentrux what-if . --move-file old/path.ts:new/path.ts --format json
sentrux what-if . --break-cycle a.ts,b.ts,c.ts --format json
```

5. Use `sentrux gate` for regression checks:

```bash
sentrux gate --save .
sentrux gate .
```

## Interpretation Rules

- Treat `debt` as advisory, not as a failing gate.
- Prefer targets that combine multiple signals, such as fan-out plus complexity or fan-in plus blast radius.
- Do not refactor a hotspot with many importers without characterization tests.
- Do not chase generated, vendored, archived, or retired files. Add them to `.sentrux/exclude`.
- If the repo has package scripts such as `pnpm sentrux:debt`, prefer those wrappers over raw CLI commands.

## Output To User

Summarize the top targets, the evidence, and the intended refactor. Include before/after Sentrux metrics after changes. Mention if Sentrux is unavailable or if scan results are noisy because exclusions are missing.
````

Run:

```bash
test -s /Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md
```

Expected: exit 0.

- [ ] **Step 2: Add global trigger guidance**

Back up `/Users/james/.codex/AGENTS.md` and check whether the Sentrux section already exists:

```bash
cp -p /Users/james/.codex/AGENTS.md "/Users/james/.codex/AGENTS.md.bak.$(date +%Y%m%d%H%M%S)"
rg -n '^## Sentrux Architecture Advisor$' /Users/james/.codex/AGENTS.md || true
```

If the section is absent, append this section to `/Users/james/.codex/AGENTS.md`. If the section already exists, use `apply_patch` to replace only that bounded section, leaving unrelated global guidance untouched.

```markdown

## Sentrux Architecture Advisor

- For architecture debt, maintainability, code smell, refactoring targets, god files, hotspots, dependency cycles, or structural regression questions, use the `sentrux-architecture-advisor` skill when available.
- Prefer repo-owned Sentrux wrappers such as `pnpm sentrux:debt` or `pnpm sentrux:check:strict`; otherwise use the local `sentrux` CLI directly.
- Treat `sentrux debt` as advisory and `sentrux gate` as the regression check.
```

Run:

```bash
rg -n "sentrux-architecture-advisor|Sentrux Architecture Advisor" /Users/james/.codex/AGENTS.md /Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md
test "$(rg -c '^## Sentrux Architecture Advisor$' /Users/james/.codex/AGENTS.md)" = "1"
```

Expected: matches in both files.

- [ ] **Step 3: Verify no global config syntax was damaged**

Run:

```bash
sed -n '1,220p' /Users/james/.codex/AGENTS.md
sed -n '1,220p' /Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md
```

Expected: both files are readable Markdown and include the Sentrux guidance.

- [ ] **Step 4: Confirm global config remains outside the fork**

The global Codex files live outside the Sentrux fork and should not be committed in this repository. Confirm the fork status without adding the global files:

```bash
git status --short
```

Expected: only reviewed fork changes appear. `/Users/james/.codex/AGENTS.md` and `/Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md` are user configuration and do not appear in this repository status.

### Task 8: Final Verification And Review Handoff

**Files:**
- Verify: Sentrux fork
- Verify: global Codex skill files

- [ ] **Step 1: Run Rust tests**

```bash
cargo check --workspace --all-targets
cargo test -p sentrux-core advisor
cargo test -p sentrux-core whatif
cargo test -p sentrux --lib
```

Expected: all three commands exit 0. Existing warnings are acceptable if no new warning is tied to the changed code.

- [ ] **Step 2: Run CLI smoke checks**

```bash
cargo run -p sentrux -- debt . --limit 3
cargo run --quiet -p sentrux -- debt . --limit 3 --format json | jq -e '.summary.quality_signal and (.targets | type == "array") and (.cycles | type == "array")' >/dev/null
cargo run --quiet -p sentrux -- diagnostics . --limit 3 --format json | jq -e '.summary.quality_signal and (.targets | type == "array")' >/dev/null
cargo run --quiet -p sentrux -- file-detail . sentrux-bin/src/main_impl.rs --format json | jq -e '.path == "sentrux-bin/src/main_impl.rs" and (.functions | type == "array")' >/dev/null
cargo run --quiet -p sentrux -- what-if . --remove-file sentrux-bin/src/main_impl.rs --format json | jq -e 'has("improved") and has("score_before") and has("score_after")' >/dev/null
```

Expected: all commands exit 0 except `file-detail` exits 1 only if the scanned path is excluded or missing. JSON commands print parseable JSON objects.

- [ ] **Step 3: Verify global skill visibility**

```bash
fd sentrux-architecture-advisor /Users/james/.codex/skills
rg -n "sentrux-architecture-advisor|sentrux debt|sentrux gate" /Users/james/.codex/AGENTS.md /Users/james/.codex/skills/sentrux-architecture-advisor/SKILL.md
```

Expected: the skill file is found and both guidance files contain Sentrux references.

- [ ] **Step 4: Check formatting and dirty state**

```bash
cargo fmt --all --check
git diff --check
git status --short
```

Expected: formatting and whitespace checks pass. `git status --short` may show only reviewed fork changes and global Codex files are outside the fork.

- [ ] **Step 5: Do not push**

```bash
git remote -v
git status --branch --short
```

Expected: branch is local or ahead locally. Do not run `git push`, do not open a pull request, and do not publish the fork until a human explicitly asks for that.

## Self-Review

- Spec coverage: CLI-first workflow is covered by Tasks 1-6. Global/user Codex guidance is covered by Task 7. No-push review handoff is covered by Task 8.
- Deferred-work scan: The plan contains no deferred implementation markers and no vague edge handling instructions.
- Type consistency: `OutputFormat`, `AdviceReport`, `FileDetailReport`, and `WhatIfAction` names are used consistently across tasks.
