use cfdcraft_logscan::{ErrorId, explain};

const SNAPPY: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/of14-snappyHexMesh.log");
const WIND_AROUND: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/of14-windaround-buildings.log");

const BANNER: &str = "trapFpe: Floating point exception trapping enabled (FOAM_SIGFPE).\n";

const TAIL_MISSING_FILE: &str =
    "--> FOAM FATAL IO ERROR:\ncannot open file /w/system/controlDict\n\nFOAM exiting\n";
const TAIL_NUMERICAL_BLOWUP: &str =
    "#1  Foam::sigFpe::sigHandler(int) at ??:?\nFloating point exception (core dumped)\n";
const TAIL_UNKNOWN_TYPE: &str =
    "--> FOAM FATAL IO ERROR:\nUnknown patchField type fixedFlux for patch inlet\n";
const TAIL_MISSING_KEYWORD: &str = "--> FOAM FATAL IO ERROR:\nkeyword nu is undefined in dictionary \"/w/constant/transportProperties\"\n";
const TAIL_LINEAR: &str = "--> FOAM FATAL ERROR:\nMaximum number of iterations exceeded\n";
const TAIL_POOR_MESH: &str = "Checking geometry...\n    Failed 2 mesh checks.\n";
const TAIL_MEMORY: &str =
    "terminate called after throwing an instance of 'std::bad_alloc'\n  what():  std::bad_alloc\n";
const TAIL_DISK: &str = "write error: No space left on device\n";
const TAIL_DECOMPOSITION: &str = "--> FOAM FATAL ERROR:\nWrong number of processor directories\n";
const TAIL_SLOTS: &str =
    "There are not enough slots available in the system to satisfy the 4 slots\n";
const TAIL_SEGFAULT: &str = "Segmentation fault (core dumped)\n";
const TAIL_UNKNOWN: &str = "Time = 12\nEnd\n";

#[test]
fn each_known_pattern_maps_to_its_id() {
    let table = [
        (TAIL_MISSING_FILE, ErrorId::MissingFile, "CFD-SOLVER-001"),
        (
            TAIL_NUMERICAL_BLOWUP,
            ErrorId::NumericalBlowup,
            "CFD-SOLVER-002",
        ),
        (TAIL_UNKNOWN_TYPE, ErrorId::UnknownType, "CFD-SOLVER-003"),
        (
            TAIL_MISSING_KEYWORD,
            ErrorId::MissingKeyword,
            "CFD-SOLVER-004",
        ),
        (
            TAIL_LINEAR,
            ErrorId::LinearSolverNotConverged,
            "CFD-SOLVER-005",
        ),
        (TAIL_POOR_MESH, ErrorId::PoorMesh, "CFD-SOLVER-006"),
        (TAIL_MEMORY, ErrorId::OutOfMemory, "CFD-SOLVER-007"),
        (TAIL_DISK, ErrorId::DiskFull, "CFD-SOLVER-008"),
        (
            TAIL_DECOMPOSITION,
            ErrorId::DecompositionMismatch,
            "CFD-SOLVER-009",
        ),
        (TAIL_SLOTS, ErrorId::NotEnoughSlots, "CFD-SOLVER-010"),
        (TAIL_SEGFAULT, ErrorId::SegmentationFault, "CFD-SOLVER-011"),
    ];
    for (tail, id, code) in table {
        assert_eq!(explain(tail), id, "tail: {tail}");
        assert_eq!(id.code(), code);
    }
}

#[test]
fn the_real_logs_map_correctly() {
    assert_eq!(explain(SNAPPY), ErrorId::MissingFile);
    assert_eq!(explain(WIND_AROUND), ErrorId::NumericalBlowup);
}

#[test]
fn an_unknown_tail_is_unknown() {
    assert_eq!(explain(TAIL_UNKNOWN), ErrorId::Unknown);
    assert_eq!(ErrorId::Unknown.code(), "CFD-SOLVER-099");
    assert_eq!(explain(""), ErrorId::Unknown);
}

#[test]
fn the_codes_are_twelve_distinct_values() {
    let all = [
        ErrorId::MissingFile,
        ErrorId::NumericalBlowup,
        ErrorId::UnknownType,
        ErrorId::MissingKeyword,
        ErrorId::LinearSolverNotConverged,
        ErrorId::PoorMesh,
        ErrorId::OutOfMemory,
        ErrorId::DiskFull,
        ErrorId::DecompositionMismatch,
        ErrorId::NotEnoughSlots,
        ErrorId::SegmentationFault,
        ErrorId::Unknown,
    ];
    let mut codes: Vec<&str> = all.iter().map(|id| id.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), 12);
}

#[test]
fn the_first_listed_pattern_wins() {
    let both = format!("{TAIL_NUMERICAL_BLOWUP}Maximum number of iterations exceeded\n");
    assert_eq!(explain(&both), ErrorId::NumericalBlowup);
}

#[test]
fn the_banner_alone_is_not_an_error() {
    assert_eq!(
        explain(&format!("{BANNER}Time = 3\nEnd\n")),
        ErrorId::Unknown
    );
    assert_eq!(
        explain("sigFpe : Enabling floating point exception trapping (FOAM_SIGFPE).\nEnd\n"),
        ErrorId::Unknown
    );
    let with_error = format!("{BANNER}{TAIL_DISK}");
    assert_eq!(explain(&with_error), ErrorId::DiskFull);
}
