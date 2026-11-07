# Log fixture provenance

The logs the tests read are built in code, read from another crate's fixtures, or are the `v2512-real-*` files in this folder. Each row says which, and whether the text is real or synthetic.

| Fixture | Where | Real or synthetic | How it was made |
|---|---|---|---|
| cavity logs (`cavity_log`, `cavity_log_with_end`, `ux_log`, `local_log`) | `tests/support/mod.rs` | synthetic | Generated in code with the line shapes of a simpleFoam log. Numbers are written in exponent form with twelve digits after the point, so the text is the same on every platform. |
| `v2512-simpleFoam-final-iteration.log` | the backend crate's fixtures, read by relative `include_str!` | real | Header and final iteration of the first real cavity run on the ESI v2512 line; intermediate iterations were left out when it was saved. It carries the harmless start-up banner about floating-point trapping. |
| `of14-windaround-buildings.log` | the backend crate's fixtures, read by relative `include_str!` | real | Output of a Foundation v14 feature-extraction tool that died on a floating-point exception. |
| `of14-snappyHexMesh.log` | the backend crate's fixtures, read by relative `include_str!` | real | Output of a Foundation v14 meshing run that stopped on a missing feature file. |
| `v2512-real-cavity-simpleFoam.log` | this folder, `tests/real_log_tests.rs` | real | The solver log of the cavity on the ESI v2512 line, run on 2026-11-02 in the pinned image (digest `sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319`), 2 CPUs. The run's runner was killed and restarted part-way and the job carried on, so the log spans that. Removed: the host name and the process id (replaced by `redacted`). Cut: the file holds the header, iterations 1 to 250 and 4751 to 5000, and `End`; iterations 251 to 4750 were left out whole, because the full log is 2.7 MB. |
| `v2512-real-diverged-simpleFoam.log` | this folder, `tests/real_log_tests.rs` | real | The log of a cavity run whose lid speed was set absurdly high, stopped by the watchdog for a growing continuity error. Same image, date and redaction. Cut: the header, iterations 1 to 250 and 2701 to 3009, and `End`; iterations 251 to 2700 were left out whole, because the full log is 1.6 MB. The first iteration is kept, so the smallest continuity error the rule compares with is the real one. The stop edit of the solver's control file shows near the end (the "Re-reading object controlDict" lines). No floating-point trap and no non-finite number occurred in this run: it was stopped on the continuity rule. |
| `v2512-real-parallel-decomposePar.log` | this folder, `tests/real_log_tests.rs` | real | `decomposePar -force` of a 300 x 300 case on 2 processes. Same image, date and redaction. Whole. |
| `v2512-real-parallel-simpleFoam.log` | this folder, `tests/real_log_tests.rs` | real | `simpleFoam -parallel` on 2 processes, 120 iterations, ending with `End` and `Finalising parallel run`. Same image, date and redaction (the host name also appears in the `Hosts` list). Whole. |
| `v2512-real-parallel-reconstructPar.log` | this folder, `tests/real_log_tests.rs` | real | `reconstructPar -latestTime` of that run. Same image, date and redaction. Whole. |
| inline log lines in the scan and divergence tests | the test files | synthetic | A few lines typed to show one rule, in the shapes above. |

## Error-table tails

The tails in `tests/explain_tests.rs` map to the known-error table. Two come from real logs. The rest are synthetic: they are typed from the solver's documented messages, and each needs a real log from a later real-host recording before it is called real.

| Tail | Where | Real or synthetic | Note |
|---|---|---|---|
| `SNAPPY` | `of14-snappyHexMesh.log` in the backend crate's fixtures | real | Gives the missing-file error. |
| `WIND_AROUND` | `of14-windaround-buildings.log` in the backend crate's fixtures | real | Gives the numerical blow-up error. |
| `TAIL_MISSING_FILE` | `tests/explain_tests.rs` | synthetic | The "cannot open file" wording; the real log has the "Could not open" wording. |
| `TAIL_NUMERICAL_BLOWUP` | `tests/explain_tests.rs` | synthetic | Two lines taken in shape from the real trap log. |
| `TAIL_UNKNOWN_TYPE` | `tests/explain_tests.rs` | synthetic | Documented wording. |
| `TAIL_MISSING_KEYWORD` | `tests/explain_tests.rs` | synthetic | Documented wording. |
| `TAIL_LINEAR` | `tests/explain_tests.rs` | synthetic | Documented wording. |
| `TAIL_POOR_MESH` | `tests/explain_tests.rs` | synthetic | The mesh-check summary line. |
| `TAIL_MEMORY` | `tests/explain_tests.rs` | synthetic | The usual C++ out-of-memory abort text. |
| `TAIL_DISK` | `tests/explain_tests.rs` | synthetic | The operating system's message. |
| `TAIL_DECOMPOSITION` | `tests/explain_tests.rs` | synthetic | Documented wording. |
| `TAIL_SLOTS` | `tests/explain_tests.rs` | synthetic | The parallel launcher's message. |
| `TAIL_SEGFAULT` | `tests/explain_tests.rs` | synthetic | The shell's message. |
| `TAIL_UNKNOWN` | `tests/explain_tests.rs` | synthetic | A healthy two-line tail that matches nothing. |
| `BANNER` | `tests/explain_tests.rs` | real | The start-up line from the real v2512 log, used to show it is not an error. |

## Not yet real

The text a real floating-point trap prints has not been recorded: the one run that blew up was stopped on the continuity rule before any number overflowed. `of14-windaround-buildings.log` stays the only real trap text, and it is from the Foundation line.
