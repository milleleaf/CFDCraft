# Fixture provenance

Most layouts the tests use are written by the helpers in `tests/support/mod.rs` and follow the layout read from the solver source: they are synthetic. The `v2512-real-*` files in this folder are real. They were written by the solver of the ESI v2512 line in the pinned image (digest `sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319`) on 2026-11-02, from the cavity run whose runner was killed and restarted part-way. They are whole and unchanged: no host name, process id or path is in them. `tests/real_file_tests.rs` reads them.

| Layout | Made by | Status |
|---|---|---|
| Probes file: `# Probe <i> (<x> <y> <z>)` headers, an index line, a `# Time` line, then rows of a time and one value or `(x y z)` vector per probe | `probes_text` | synthetic |
| Probes file of one probe, 5000 rows (`v2512-real-probe_4-U.dat`): the header `# Probe 0 (0.025 0.075 0.025)`, a `# Time` line with the index, then a time and `(Ux Uy Uz)` per row. Each probe of the cavity is its own function object and its own file. | the solver | real |
| Surface field value file: `# Region type`, `# Faces`, `# Area`, `# Scale factor`, a `# Time` line naming the columns, then tab separated rows | `surface_text` | synthetic |
| Surface field value file (`v2512-real-surfaceFieldValue-movingWall.dat`): the same five header lines, one column `sum(phi)`, 5000 rows, every value exactly zero (a closed cavity) | the solver | real |
| Sampled line, plain numeric rows: coordinate columns first, then the field components, no header | `xy_text` | synthetic |
| Sampled line of the built-in centre lines (`v2512-real-centreline-vertical_U.xy`): written with `axis y`, so four columns per row, the coordinate along the line and then Ux Uy Uz, 64 rows, no header | the solver | real |

The writer's own line monitors use `axis xyz`, which gives three coordinate columns and then the components (six columns for a velocity). No run has written such a file yet, so that layout stays synthetic.
