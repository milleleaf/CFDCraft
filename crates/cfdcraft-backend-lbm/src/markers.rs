//! Facts about the solver's wire format that no real run has shown yet. Each constant says what
//! it assumes. Every one is `UNPROBED` until a hosted run of the pinned image has shown it, and a
//! mismatch found there is a reviewed change of the constant, not a change of the writer.

/// UNPROBED: the protocol revision named in the opening request. The solver's documentation lists
/// 2025-06-18, 2025-03-26 and 2024-11-05.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// UNPROBED: the tool that runs several commands in order and stops at the first error.
pub const BATCH_TOOL: &str = "batch";

/// UNPROBED: the tool that runs one command.
pub const EXECUTE_TOOL: &str = "execute";

/// UNPROBED: how many cells thick the slab is that stands for a two-dimensional study. The
/// documentation does not give the least thickness that two slip faces accept.
pub const SLAB_CELLS_Z: u32 = 3;

/// UNPROBED: the server ends its process when its input ends, after answering every line. The
/// run script relies on it, because nothing else tells the server that the stream is complete.
pub const EXITS_AT_END_OF_INPUT: bool = true;

/// UNPROBED: the member of a solve reply that holds the number of time steps taken.
pub const REPLY_STEPS: &str = "steps";

/// UNPROBED: the member of a solve reply that holds why a run with a steady rule stopped.
pub const REPLY_STOP: &str = "stop";

/// UNPROBED: the member of a solve reply that holds the change of the fluid mass.
pub const REPLY_MASS: &str = "mass";

/// UNPROBED: the member of a solve reply that holds the warnings of the setup and the run.
pub const REPLY_WARNINGS: &str = "warnings";

/// The most that is read of any file of a run, in bytes. A longer file is read only up to this
/// size, and a report says so; a view never serves a series built from a file that was longer.
pub const READ_CAP_BYTES: u64 = 8 * 1024 * 1024;

/// UNPROBED: the command that runs the solve.
pub const SOLVE_COMMAND: &str = "solve.fluid";

/// UNPROBED: the command that reads points, lines, forces and heat out of a solved step.
pub const PROBE_COMMAND: &str = "solve.probe";

/// UNPROBED: the member of a reply that holds the answer of the call, and the members around it
/// that mark an error. A reply with a top-level `error`, or a result flagged as an error, is an
/// error reply. The answer is the structured content when there is one, else the text of the
/// first content item read as JSON, else the result itself.
pub const ENVELOPE_RESULT: &str = "result";
pub const ENVELOPE_ERROR: &str = "error";
pub const ENVELOPE_IS_ERROR: &str = "isError";
pub const ENVELOPE_STRUCTURED: &str = "structuredContent";
pub const ENVELOPE_CONTENT: &str = "content";
pub const ENVELOPE_TEXT: &str = "text";

/// UNPROBED: the member of the mass reply that holds the change of the fluid mass over the run,
/// relative to the mass at the start.
pub const REPLY_MASS_CHANGE: &str = "relative_change";

/// UNPROBED: the largest relative change of the fluid mass that a good run shows. The mass of a
/// closed box does not change; an open channel starting from rest changes a little while the
/// first pressure waves leave.
pub const MASS_CHANGE_LIMIT: f64 = 1e-3;

/// UNPROBED: the stop reasons of a run that ended as asked: at its end time, after its steps, or
/// because the steady rule found the flow settled. Any other reason is a failure.
pub const NORMAL_STOPS: [&str; 3] = ["end_time", "steps", "steady"];

/// UNPROBED: the stop reason of a run ended by the steady rule, which takes fewer steps than a
/// request that names a step count.
pub const STEADY_STOP: &str = "steady";

/// UNPROBED: a word that marks a warning about an unstable flow, compared without regard to case.
pub const INSTABILITY_WORD: &str = "unstable";

/// UNPROBED: the server writes at least one line of its own to the log. A log with no text at all
/// means the process did not start, or was not the server.
pub const LOG_HOLDS_TEXT: bool = true;

/// UNPROBED: the member of a solve reply that holds the change measure of the steady rule: the
/// relative change of the flow over each window of flow time, oldest first. A single number counts
/// as a series of one.
pub const REPLY_CHANGE: &str = "change";

/// UNPROBED: the members of a probe reply.
/// - points: `points` holds one entry per asked point, with `cell` (null outside the grid),
///   `solid`, `velocity` (three numbers) and `pressure` (gauge, in the display units); the frame's
///   flow time is the member `time`.
/// - line: `xyz` holds the sample points and `values` the field at them.
/// - forces: `times`, and `forces` holds, for each wall name, one `[x, y, z]` force per time.
/// - heat: `times`, and `heat` holds, for each wall name, the power into the wall per time.
pub const REPLY_TIME: &str = "time";
pub const REPLY_TIMES: &str = "times";
pub const REPLY_POINTS: &str = "points";
pub const REPLY_CELL: &str = "cell";
pub const REPLY_SOLID: &str = "solid";
pub const REPLY_VELOCITY: &str = "velocity";
pub const REPLY_PRESSURE: &str = "pressure";
pub const REPLY_XYZ: &str = "xyz";
pub const REPLY_VALUES: &str = "values";
pub const REPLY_FORCES: &str = "forces";
pub const REPLY_HEAT: &str = "heat";

/// UNPROBED: the line fields of a probe request that give the three velocity components.
pub const FIELD_VELOCITY: [&str; 3] = ["velocity_x", "velocity_y", "velocity_z"];

/// UNPROBED: the members of a probe request. `step` names the step that was solved. A call holds
/// exactly one of `points`, `line`, `forces` and `heat`; `frame` (one to the number of frames the
/// step records) goes with `points`. The manual's page on results gives these forms.
pub const PARAM_STEP: &str = "step";
pub const PARAM_POINTS: &str = "points";
pub const PARAM_LINE: &str = "line";
pub const PARAM_FORCES: &str = "forces";
pub const PARAM_HEAT: &str = "heat";
pub const PARAM_FRAME: &str = "frame";

/// UNPROBED: the members of the object a line probe holds. `count` is the number of sample points
/// from `from` to `to`, both ends included, and `field` is one of [`FIELD_VELOCITY`].
pub const LINE_FROM: &str = "from";
pub const LINE_TO: &str = "to";
pub const LINE_COUNT: &str = "count";
pub const LINE_FIELD: &str = "field";

/// UNPROBED: the member of a step's procedure that sets how many frames the run records for the
/// probes to read afterwards.
pub const STEP_FRAMES: &str = "frames";

/// UNPROBED: the name the solver gives the one face of an imported surface that has no tag. A
/// force probe names a wall of a body as `<body>/<face>`.
pub const BODY_FACE: &str = "face0";

/// UNPROBED: the solver's bounds on a probe request, from its manual: points in one call, walls
/// in one force or heat call, and sample points on one line.
pub const MAX_PROBE_POINTS: usize = 10_000;
pub const MAX_PROBE_WALLS: usize = 100;
pub const MAX_LINE_COUNT: u64 = 10_000;

/// UNPROBED: the most frames a step records (the manual gives one to one thousand).
pub const MAX_FRAMES: u64 = 1000;

/// The fewest frames a run records. A steady run records this many, and a transient run is never
/// given fewer.
pub const MIN_FRAMES: u64 = 10;

/// UNPROBED: the bytes the frames of a run may take, half of the solver's default cap on the
/// memory the recorded frames use.
pub const FRAME_MEMORY_BYTES: u64 = 1_000_000_000;

/// UNPROBED: the bytes one frame takes for one cell of a run with heat, as the manual states.
pub const FRAME_BYTES_PER_CELL: u64 = 20;
