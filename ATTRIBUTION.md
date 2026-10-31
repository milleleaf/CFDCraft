# Attribution

Every bundled third-party item in this repository, with its author, source and licence. CFDCraft's own code is MIT OR Apache-2.0 (see [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE)). Rust dependencies come from crates.io and keep their own licences; the allowed licences are listed in `deny.toml` and checked in CI.

| Path | What | Author | Source | Licence |
|---|---|---|---|---|
| `crates/cfdcraft-bench-ghia/data/ghia_re100.csv` | Lid-driven cavity centreline reference values, Re=100 | U. Ghia, K. N. Ghia and C. T. Shin, J. Comput. Phys. 48 (1982) 387-411; table vendored from triscale-innov/LidJul.jl | <https://github.com/triscale-innov/LidJul.jl/blob/e5354b7e5eb8567ac812c204338dba2cf0ef540c/validation/ghia_re100.csv> | MIT (Expat); the upstream notice is at the top of the file |
| `crates/cfdcraft-backend-esi/tests/fixtures/*.log` | Captured OpenFOAM program output used as test inputs | Output of OpenFOAM runs made for this project | n/a | Program output; not OpenFOAM source code |

The Ghia values were checked against the Sandia SIERRA/Fuego Verification Manual v5.20, Table 22.1-1 (<https://www.sandia.gov/app/uploads/sites/315/2025/11/Fuego_Verification_5_20.pdf>).
