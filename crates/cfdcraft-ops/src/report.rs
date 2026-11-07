//! The one structured result every operation returns, and the exit class derived from it.

use crate::ghia::GateReport;
use cfdcraft_model::{Diagnostic, MigrationNote, Severity, render_text};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How an operation ended. The numbers are the process exit status and never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    /// Every check passed.
    Passed,
    /// A quality gate on a run failed.
    GateFailed,
    /// The study, or the way the tool was asked to use it, is invalid.
    StudyInvalid,
    /// The safety scan or the read-back check refused a case.
    GuardRefused,
    /// The machine, network or container engine failed.
    Infrastructure,
}

impl ExitClass {
    pub fn code(self) -> u8 {
        match self {
            Self::Passed => 0,
            Self::GateFailed => 1,
            Self::StudyInvalid => 2,
            Self::GuardRefused => 3,
            Self::Infrastructure => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::GateFailed => "gate_failed",
            Self::StudyInvalid => "study_invalid",
            Self::GuardRefused => "guard_refused",
            Self::Infrastructure => "infrastructure",
        }
    }

    /// The class a list of findings stands for. Errors from the guard or read-back families
    /// refuse a written case; any other error makes the input invalid; warnings never count.
    pub fn of_findings(found: &[Diagnostic]) -> Self {
        let mut errors = found.iter().filter(|d| d.severity == Severity::Error);
        if errors
            .clone()
            .any(|d| d.code.starts_with("CFD-GUARD-") || d.code.starts_with("CFD-RT-"))
        {
            Self::GuardRefused
        } else if errors.next().is_some() {
            Self::StudyInvalid
        } else {
            Self::Passed
        }
    }
}

/// A file an operation read or wrote, by kind and path relative to the case root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Artifact {
    pub kind: String,
    pub name: String,
}

/// Facts about the job that ran. Nothing here names the machine or its directories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunInfo {
    pub run_id: String,
    pub container: String,
    pub duration_seconds: f64,
    /// The exit status of the solver script. A diagnostic number only: the verdict is judged from
    /// the evidence the run left behind and never from this.
    pub process_exit: Option<i32>,
}

/// The verdict judged from the run's logs and output directories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceReport {
    /// `passed`, `failed` or `smoke_passed`. A smoke pass says the case loaded and iterated and
    /// nothing more.
    pub verdict: String,
    pub reasons: Vec<String>,
}

/// Who holds the compute host, as the holder file recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HolderInfo {
    pub run_id: String,
    pub container: String,
    pub started_epoch: u64,
    pub cap_seconds: u64,
}

/// An infrastructure failure: a stable code and a fixed public sentence. The executor's full error
/// text can name the key, the host and remote directories, so it is never stored here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InfraReport {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<HolderInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    /// The holder is far past its own cap. Reported only; a lock is never broken.
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub operation: String,
    pub exit_class: u8,
    pub exit_class_name: String,
    /// `ok` or `failed`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub study_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<MigrationNote>,
    /// Errors and warnings in one list.
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<EvidenceReport>,
    /// The reference comparison, for validate runs whose evidence passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infra: Option<InfraReport>,
}

impl Report {
    /// A passing report with nothing in it yet.
    pub fn new(operation: &str) -> Self {
        Self::from_diagnostics(operation, Vec::new(), None)
    }

    /// The class comes from the findings alone.
    pub fn from_diagnostics(
        operation: &str,
        diagnostics: Vec<Diagnostic>,
        study_hash: Option<String>,
    ) -> Self {
        let mut report = Self {
            operation: operation.to_owned(),
            exit_class: 0,
            exit_class_name: String::new(),
            status: String::new(),
            study_hash,
            migration: None,
            diagnostics,
            artifacts: Vec::new(),
            run: None,
            evidence: None,
            gate: None,
            infra: None,
        };
        report.set_class(ExitClass::of_findings(&report.diagnostics));
        report
    }

    pub fn set_class(&mut self, class: ExitClass) {
        self.exit_class = class.code();
        self.exit_class_name = class.name().to_owned();
        self.status = if class == ExitClass::Passed {
            "ok"
        } else {
            "failed"
        }
        .to_owned();
    }

    pub fn class(&self) -> ExitClass {
        match self.exit_class {
            0 => ExitClass::Passed,
            1 => ExitClass::GateFailed,
            2 => ExitClass::StudyInvalid,
            3 => ExitClass::GuardRefused,
            _ => ExitClass::Infrastructure,
        }
    }

    #[must_use]
    pub fn with_migration(mut self, note: Option<MigrationNote>) -> Self {
        self.migration = note;
        self
    }

    #[must_use]
    pub fn with_artifact(mut self, kind: &str, name: impl Into<String>) -> Self {
        self.artifacts.push(Artifact {
            kind: kind.to_owned(),
            name: name.into(),
        });
        self
    }

    #[must_use]
    pub fn for_operation(mut self, operation: &str) -> Self {
        self.operation = operation.to_owned();
        self
    }

    /// The migration note, then every finding, for a person reading a terminal.
    pub fn render_text(&self) -> String {
        let note = self
            .migration
            .as_ref()
            .map_or_else(String::new, |n| format!("note: {}\n", n.summary));
        let mut out = note + &render_text(&self.diagnostics);
        if let Some(e) = &self.evidence {
            out += &e.render_text(self.operation == "smoke");
        }
        if let Some(g) = &self.gate {
            out += &g.render_text();
        }
        out
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a report is plain data")
    }
}

impl EvidenceReport {
    /// The verdict line, its reasons and, for a smoke run, the sentence that says what it shows.
    pub fn render_text(&self, smoke: bool) -> String {
        let mut out = match self.verdict.as_str() {
            "passed" => "evidence verdict: PASSED\n".to_owned(),
            "smoke_passed" => "evidence verdict: SMOKE PASSED\n".to_owned(),
            _ => "evidence verdict: FAILED\n".to_owned(),
        };
        for reason in &self.reasons {
            out += &format!("  {reason}\n");
        }
        if smoke {
            out += "smoke run: shows only that the case loads and iterates; convergence is not claimed\n";
        }
        out
    }
}
