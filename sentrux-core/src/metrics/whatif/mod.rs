//! What-if simulator — predict architectural impact of hypothetical changes.
//!
//! "What if I move this file?" / "What if I add this dependency?" / "What if I break this cycle?"
//! → Simulates the change on a cloned graph, recomputes metrics, shows before/after diff.
//!
//! Zero side effects: operates on cloned data only.

use super::arch;
use crate::core::types::ImportEdge;
use std::collections::HashMap;

// ── Public types ──

/// A hypothetical change to simulate against the dependency graph.
/// Each variant describes a single atomic graph mutation.
#[derive(Debug, Clone)]
pub enum WhatIfAction {
    /// Move/rename a file: rewrites all edges referencing old_path → new_path.
    /// Simulates refactoring a file to a different module without changing behavior.
    MoveFile { old_path: String, new_path: String },

    /// Add a new import edge between two files.
    /// Simulates adding a dependency (e.g., importing a utility module).
    AddEdge { from: String, to: String },

    /// Remove an import edge between two files.
    /// Simulates decoupling (removing an import statement).
    RemoveEdge { from: String, to: String },

    /// Remove a file entirely along with all its incoming and outgoing edges.
    /// Simulates deleting a file from the codebase.
    RemoveFile { path: String },

    /// Break a specific cycle by removing the weakest edge in the cycle.
    /// The weakest edge is the one whose removal reduces blast radius the most.
    BreakCycle { files: Vec<String> },
}

/// Result of a what-if simulation — before/after comparison of architecture metrics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WhatIfResult {
    /// Human-readable description of the simulated action
    pub action_description: String,

    // ── Before state ──
    /// Architecture score before the simulated change
    pub score_before: f64,
    /// Maximum dependency depth before
    pub max_level_before: u32,
    /// Upward dependency violation count before
    pub upward_violations_before: usize,
    /// Maximum blast radius before
    pub max_blast_before: u32,

    // ── After state ──
    /// Architecture score after the simulated change
    pub score_after: f64,
    /// Maximum dependency depth after
    pub max_level_after: u32,
    /// Upward dependency violation count after
    pub upward_violations_after: usize,
    /// Maximum blast radius after
    pub max_blast_after: u32,

    // ── Delta ──
    /// Whether the change improved the architecture score
    pub improved: bool,
    /// Human-readable change descriptions
    pub changes: Vec<String>,

    /// Per-file level changes (only files whose level changed)
    pub level_changes: Vec<LevelChange>,
}

/// A single file's level change from a what-if simulation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LevelChange {
    /// File path that changed level
    pub file: String,
    /// Dependency level before the simulated action
    pub level_before: u32,
    /// Dependency level after the simulated action
    pub level_after: u32,
}

// ── Public API ──

/// Run a what-if simulation: apply action to a cloned graph, compare before/after.
pub fn simulate(
    edges: &[ImportEdge],
    _entry_points: &[crate::core::types::EntryPoint],
    action: &WhatIfAction,
) -> WhatIfResult {
    let before = compute_arch_snapshot(edges);
    let (new_edges, description) = apply_action(edges, action);
    let after = compute_arch_snapshot(&new_edges);

    build_whatif_result(description, &before, &after, edges.len(), new_edges.len())
}

#[allow(dead_code)] // Available for compound what-if from MCP
pub fn simulate_sequence(
    edges: &[ImportEdge],
    _entry_points: &[crate::core::types::EntryPoint],
    actions: &[WhatIfAction],
) -> WhatIfResult {
    if actions.is_empty() {
        return build_noop_result(edges);
    }

    let mut current_edges = edges.to_vec();
    let mut descriptions = Vec::new();

    for action in actions {
        let (new_edges, desc) = apply_action(&current_edges, action);
        current_edges = new_edges;
        descriptions.push(desc);
    }

    let before = compute_arch_snapshot(edges);
    let after = compute_arch_snapshot(&current_edges);
    let description = descriptions.join(" \u{2192} ");

    build_whatif_result(description, &before, &after, edges.len(), current_edges.len())
}

#[allow(dead_code)]
fn build_noop_result(edges: &[ImportEdge]) -> WhatIfResult {
    let snap = compute_arch_snapshot(edges);
    WhatIfResult {
        action_description: "(no changes)".to_string(),
        score_before: snap.score,
        max_level_before: snap.max_level,
        upward_violations_before: snap.violation_count,
        max_blast_before: snap.max_blast,
        score_after: snap.score,
        max_level_after: snap.max_level,
        upward_violations_after: snap.violation_count,
        max_blast_after: snap.max_blast,
        improved: false,
        changes: Vec::new(),
        level_changes: Vec::new(),
    }
}

/// Snapshot of architecture metrics at a single point in time.
struct ArchMetricSnapshot {
    levels: HashMap<String, u32>,
    max_level: u32,
    violation_count: usize,
    max_blast: u32,
    score: f64,
}

/// Compute all architecture metrics for a set of edges.
fn compute_arch_snapshot(edges: &[ImportEdge]) -> ArchMetricSnapshot {
    let (levels, max_level) = arch::compute_levels(edges);
    let violations = arch::find_upward_violations(edges, &levels);
    let blast = arch::compute_blast_radius(edges);
    let max_blast = blast.values().copied().max().unwrap_or(0);
    let ratio = if edges.is_empty() { 0.0 } else { violations.len() as f64 / edges.len() as f64 };
    let score = score_from_ratio(ratio);
    ArchMetricSnapshot { levels, max_level, violation_count: violations.len(), max_blast, score }
}

/// Build a WhatIfResult from before/after snapshots and edge counts.
fn build_whatif_result(
    description: String,
    before: &ArchMetricSnapshot,
    after: &ArchMetricSnapshot,
    edge_count_before: usize,
    edge_count_after: usize,
) -> WhatIfResult {
    let mut changes = Vec::new();
    compare_metric(&mut changes, "Max level", before.max_level, after.max_level);
    compare_metric(&mut changes, "Upward violations", before.violation_count as u32, after.violation_count as u32);
    compare_metric(&mut changes, "Max blast radius", before.max_blast, after.max_blast);
    compare_metric(&mut changes, "Total edges", edge_count_before as u32, edge_count_after as u32);

    let level_changes = compute_level_changes(&before.levels, &after.levels);

    let not_worse = after.score >= before.score - f64::EPSILON
        && after.violation_count <= before.violation_count
        && after.max_blast <= before.max_blast;
    let strictly_better = after.score > before.score + f64::EPSILON
        || after.violation_count < before.violation_count
        || after.max_blast < before.max_blast
        || after.max_level < before.max_level
        || edge_count_after < edge_count_before;
    let improved = not_worse && strictly_better;

    WhatIfResult {
        action_description: description,
        score_before: before.score,
        max_level_before: before.max_level,
        upward_violations_before: before.violation_count,
        max_blast_before: before.max_blast,
        score_after: after.score,
        max_level_after: after.max_level,
        upward_violations_after: after.violation_count,
        max_blast_after: after.max_blast,
        improved,
        changes,
        level_changes,
    }
}

/// Find the best edge to remove from a cycle to minimize architectural impact.
/// Uses fan-in heuristic: remove the edge whose target has the lowest fan-in
/// (least depended-on file), breaking the cycle with minimal disruption.
/// This avoids the previous O(E×V²) approach of computing full blast radius
/// per candidate edge.
pub fn find_best_cycle_break(
    edges: &[ImportEdge],
    cycle_files: &[String],
) -> Option<(String, String)> {
    let cycle_set: std::collections::HashSet<&str> =
        cycle_files.iter().map(|s| s.as_str()).collect();

    let cycle_edges: Vec<&ImportEdge> = edges
        .iter()
        .filter(|e| cycle_set.contains(e.from_file.as_str()) && cycle_set.contains(e.to_file.as_str()))
        .collect();

    if cycle_edges.is_empty() {
        return None;
    }

    // Build fan-in counts for cycle files — edge with lowest target fan-in
    // is the weakest link (fewest other files depend on that import).
    let mut fan_in: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for e in edges {
        *fan_in.entry(e.to_file.as_str()).or_default() += 1;
    }

    let best = cycle_edges.iter()
        .min_by_key(|e| fan_in.get(e.to_file.as_str()).copied().unwrap_or(0));

    best.map(|e| (e.from_file.clone(), e.to_file.clone()))
}

// ── Internal helpers ──

fn apply_action(edges: &[ImportEdge], action: &WhatIfAction) -> (Vec<ImportEdge>, String) {
    match action {
        WhatIfAction::MoveFile { old_path, new_path } => apply_move(edges, old_path, new_path),
        WhatIfAction::AddEdge { from, to } => apply_add_edge(edges, from, to),
        WhatIfAction::RemoveEdge { from, to } => apply_remove_edge(edges, from, to),
        WhatIfAction::RemoveFile { path } => apply_remove_file(edges, path),
        WhatIfAction::BreakCycle { files } => apply_break_cycle(edges, files),
    }
}

fn apply_move(edges: &[ImportEdge], old_path: &str, new_path: &str) -> (Vec<ImportEdge>, String) {
    let new_edges: Vec<ImportEdge> = edges
        .iter()
        .map(|e| {
            let from = if e.from_file == old_path { new_path.to_string() } else { e.from_file.clone() };
            let to = if e.to_file == old_path { new_path.to_string() } else { e.to_file.clone() };
            ImportEdge { from_file: from, to_file: to }
        })
        .collect();
    (new_edges, format!("Move {old_path} \u{2192} {new_path}"))
}

fn apply_add_edge(edges: &[ImportEdge], from: &str, to: &str) -> (Vec<ImportEdge>, String) {
    let mut new_edges = edges.to_vec();
    new_edges.push(ImportEdge { from_file: from.to_string(), to_file: to.to_string() });
    (new_edges, format!("Add edge {from} \u{2192} {to}"))
}

fn apply_remove_edge(edges: &[ImportEdge], from: &str, to: &str) -> (Vec<ImportEdge>, String) {
    let new_edges: Vec<ImportEdge> = edges
        .iter()
        .filter(|e| !(e.from_file == from && e.to_file == to))
        .cloned()
        .collect();
    (new_edges, format!("Remove edge {from} \u{2192} {to}"))
}

fn apply_remove_file(edges: &[ImportEdge], path: &str) -> (Vec<ImportEdge>, String) {
    let new_edges: Vec<ImportEdge> = edges
        .iter()
        .filter(|e| e.from_file != path && e.to_file != path)
        .cloned()
        .collect();
    (new_edges, format!("Remove file {path}"))
}

fn apply_break_cycle(edges: &[ImportEdge], files: &[String]) -> (Vec<ImportEdge>, String) {
    if let Some((from, to)) = find_best_cycle_break(edges, files) {
        let new_edges: Vec<ImportEdge> = edges
            .iter()
            .filter(|e| !(e.from_file == from && e.to_file == to))
            .cloned()
            .collect();
        (new_edges, format!("Break cycle: remove {from} \u{2192} {to}"))
    } else {
        (edges.to_vec(), "No cycle edge found to break".to_string())
    }
}

fn compare_metric(changes: &mut Vec<String>, name: &str, before: u32, after: u32) {
    if before != after {
        let arrow = if after < before { "↓" } else { "↑" };
        changes.push(format!("{name}: {before} → {after} {arrow}"));
    }
}

fn compute_level_changes(
    before: &HashMap<String, u32>,
    after: &HashMap<String, u32>,
) -> Vec<LevelChange> {
    let mut changes = Vec::new();

    // Check all files in either map
    let mut all_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for k in before.keys() {
        all_files.insert(k.as_str());
    }
    for k in after.keys() {
        all_files.insert(k.as_str());
    }

    for file in all_files {
        let lb = before.get(file).copied().unwrap_or(0);
        let la = after.get(file).copied().unwrap_or(0);
        if lb != la {
            changes.push(LevelChange {
                file: file.to_string(),
                level_before: lb,
                level_after: la,
            });
        }
    }

    // Sort by absolute delta descending (largest changes first)
    changes.sort_by(|a, b| {
        let a_delta = (a.level_before as i64 - a.level_after as i64).abs();
        let b_delta = (b.level_before as i64 - b.level_after as i64).abs();
        b_delta.cmp(&a_delta).then_with(|| a.file.cmp(&b.file))
    });

    changes
}

/// Delegate to arch::score_levelization to keep thresholds in a single place.
fn score_from_ratio(upward_ratio: f64) -> f64 {
    crate::metrics::arch::score_levelization(upward_ratio)
}

#[cfg(test)]
mod tests;
