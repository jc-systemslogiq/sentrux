//! Architecture advisor reports for CLI and agent consumption.

use super::arch::ArchReport;
use super::{FileMetric, FuncMetric, HealthReport};
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
        evidence.insert("fan_out".into(), serde_json::json!(metric.value));
        if let Some(cc) = complex_by_file.get(&metric.path) {
            evidence.insert("max_cyclomatic_complexity".into(), serde_json::json!(cc));
        }
        if let Some(lines) = long_by_file.get(&metric.path) {
            evidence.insert("max_function_lines".into(), serde_json::json!(lines));
        }
        upsert_target(&mut index, AdviceTarget {
            category: AdviceCategory::GodFile,
            path: metric.path.clone(),
            symbol: None,
            priority: metric.value as f64
                + complex_by_file.get(&metric.path).copied().unwrap_or(0) as f64 * 0.25,
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
        evidence.insert("fan_in".into(), serde_json::json!(metric.value));
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
        evidence.insert("cyclomatic_complexity".into(), serde_json::json!(metric.value));
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
        evidence.insert("function_lines".into(), serde_json::json!(metric.value));
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
