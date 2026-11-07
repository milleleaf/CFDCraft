//! A small, valid-looking results folder for tests, so no test needs a solver or a container.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

/// What the fixture folder should look like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixtureSpec {
    /// The end time: names the field folder and the `Time = ` line of the solver log.
    pub end_time: u64,
    /// The cell count the logs and mesh files report.
    pub cells: u64,
    /// How many rows each centreline sample file holds.
    pub samples: usize,
    /// Added to every sampled velocity, so two folders can differ in their samples only.
    pub shift: f64,
}

/// Write the folder a good run of the cavity study would leave: the five mesh files (different
/// from one another, each naming the cell count), the three logs, the final `U` and `p`, and the
/// two centreline sample files whose velocity is `shift + index * 0.01`. The logs pass the evidence
/// check for the cavity study; 0.0005 in the mesh log is the volume of that study's domain.
pub fn write_results(dir: &Path, spec: &FixtureSpec) -> io::Result<()> {
    let end = spec.end_time.to_string();
    let mesh = dir.join("constant/polyMesh");
    let time = dir.join(&end);
    let samples = dir.join("postProcessing/centreline").join(&end);
    for folder in [&mesh, &time, &samples] {
        fs::create_dir_all(folder)?;
    }

    for name in ["points", "faces", "owner", "neighbour", "boundary"] {
        let text = format!(
            "// {name} of a fixture mesh with {} cells\n{name} {}\n",
            spec.cells,
            spec.cells + name.len() as u64
        );
        fs::write(mesh.join(name), text)?;
    }

    let cells = spec.cells;
    fs::write(
        dir.join("log.blockMesh"),
        format!("Created {cells} cells\nEnd\n"),
    )?;
    fs::write(
        dir.join("log.checkMesh"),
        format!("cells: {cells}\nTotal volume = 0.0005\nMesh OK.\nEnd\n"),
    )?;
    fs::write(
        dir.join("log.simpleFoam"),
        format!(
            "Time = {end}\n\
             smoothSolver: Solving for Ux, Initial residual = 1e-07, Final residual = 1e-11, No Iterations 1\n\
             smoothSolver: Solving for Uy, Initial residual = 1.5e-07, Final residual = 1e-11, No Iterations 1\n\
             PCG: Solving for p, Initial residual = 2e-07, Final residual = 1e-11, No Iterations 1\n\
             ExecutionTime = 1 s ClockTime = 1 s\n\
             End\n"
        ),
    )?;

    fs::write(time.join("U"), "field U\n")?;
    fs::write(time.join("p"), "field p\n")?;

    let mut vertical = String::new();
    let mut horizontal = String::new();
    for index in 0..spec.samples {
        let coordinate = index as f64 * 0.01;
        let value = spec.shift + (index as f64 * 0.01);
        let _ = writeln!(
            vertical,
            "{coordinate:.12} {value:.12} {:.12} {:.12}",
            0.0, 0.0
        );
        let _ = writeln!(
            horizontal,
            "{coordinate:.12} {:.12} {value:.12} {:.12}",
            0.0, 0.0
        );
    }
    fs::write(samples.join("vertical_U.xy"), vertical)?;
    fs::write(samples.join("horizontal_U.xy"), horizontal)?;
    Ok(())
}
