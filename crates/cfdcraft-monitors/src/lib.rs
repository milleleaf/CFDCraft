//! The files a solver writes for probes, surface integrals and sampled lines, read in memory into named series of (time, value).
//!
//! Everything here is pure: text in, series out. A number that is not finite, a row of the wrong width, an oversized file or too many rows is refused by name and never turned into a number.

mod probes;
mod series;
mod surface;
mod xy;

pub use probes::{FieldKind, ProbeFile, ProbeInfo, Row, Value, parse_probes, probe_series};
pub use series::{
    MAX_BYTES, MAX_PROBES, MAX_ROWS, MonitorError, Series, join_series, line_mean_series,
};
pub use surface::{SurfaceFile, parse_surface, surface_series};
pub use xy::{LineFile, parse_xy};
