//! The three admission levels and how the columns of a table add up to one answer.
//!
//! A backend is admitted only when golden compile, reference oracle and parity all pass for every
//! column that counts. Nothing here reads or changes a tolerance.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Pass,
    Fail,
    NotRun,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::NotRun => "not_run",
        }
    }
}

/// One answer per level, for a column or for the whole table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Levels {
    pub golden: Level,
    pub oracle: Level,
    pub parity: Level,
}

impl Levels {
    pub fn not_run() -> Self {
        Self {
            golden: Level::NotRun,
            oracle: Level::NotRun,
            parity: Level::NotRun,
        }
    }

    /// True when all three levels passed.
    pub fn all_pass(&self) -> bool {
        self.golden == Level::Pass && self.oracle == Level::Pass && self.parity == Level::Pass
    }
}

/// The levels of one column: `oracle` is the column's own gate result.
pub(crate) fn column_levels(golden: Level, gate_passed: bool, parity: Level) -> Levels {
    Levels {
        golden,
        oracle: if gate_passed {
            Level::Pass
        } else {
            Level::Fail
        },
        parity,
    }
}

/// One level over the columns that count: nothing counted is `not_run`, a failure wins over a
/// level that did not run, and `pass` needs every counted column to pass.
fn fold(levels: &[Level]) -> Level {
    if levels.is_empty() {
        Level::NotRun
    } else if levels.contains(&Level::Fail) {
        Level::Fail
    } else if levels.contains(&Level::NotRun) {
        Level::NotRun
    } else {
        Level::Pass
    }
}

/// The levels of the table, over the columns that count.
pub(crate) fn aggregate(counted: &[Levels]) -> Levels {
    let golden: Vec<Level> = counted.iter().map(|l| l.golden).collect();
    let oracle: Vec<Level> = counted.iter().map(|l| l.oracle).collect();
    let parity: Vec<Level> = counted.iter().map(|l| l.parity).collect();
    Levels {
        golden: fold(&golden),
        oracle: fold(&oracle),
        parity: fold(&parity),
    }
}
