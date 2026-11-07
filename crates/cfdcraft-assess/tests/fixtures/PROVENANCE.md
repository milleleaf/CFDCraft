# Provenance of the test inputs

Every input of these tests is synthetic. No solver ran and no file was copied from a real case.

The generators are in `tests/support/mod.rs`.

- `log` writes a cavity log whose initial residuals fall by one percent each iteration from 1e-2, with the three solve lines of a steady incompressible run per iteration and an optional `End` line.
- `blown_up_log` writes two iterations whose pressure residual jumps from 1 to 1e7.
- `velocity_probe` and `pressure_probe` write probe files in the layout the solver writes: a location header, an index line, a time line, then a time and one value per row. The values approach a base value, ramp, or jitter, depending on the shape asked for.
- `surface_file` and `settling_surface` write surface field value files with the region header and one column.
- `line_file` and `line_folders` write sampled lines of sixteen points in the plain numeric layout, one folder per write time.
- The energy tests (`tests/energy_tests.rs`) build their surface files inline from synthetic text: the region header, a `# Time` line naming `areaIntegrate(gradT)`, and sixty rows of one vector. The sign and the axis they pin (hot and cold are both the temperature-gradient integral along the same axis, so a closed cavity gives two equal values) are a convention, not something read from a real solver file.
- The periodic tests (`tests/periodic_assess_tests.rs`) build their coefficient and probe files inline from synthetic text: a force coefficient header with the drag and lift directions, a `# Time Cd Cl` line, and rows of a sine, a growing sine or a constant sampled every millisecond.
