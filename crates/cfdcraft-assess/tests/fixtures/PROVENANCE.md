# Provenance of the test inputs

Every input of these tests is synthetic. No solver ran and no file was copied from a real case.

The generators are in `tests/support/mod.rs`.

- `log` writes a cavity log whose initial residuals fall by one percent each iteration from 1e-2, with the three solve lines of a steady incompressible run per iteration and an optional `End` line.
- `blown_up_log` writes two iterations whose pressure residual jumps from 1 to 1e7.
- `velocity_probe` and `pressure_probe` write probe files in the layout the solver writes: a location header, an index line, a time line, then a time and one value per row. The values approach a base value, ramp, or jitter, depending on the shape asked for.
- `surface_file` and `settling_surface` write surface field value files with the region header and one column.
- `line_file` and `line_folders` write sampled lines of sixteen points in the plain numeric layout, one folder per write time.
