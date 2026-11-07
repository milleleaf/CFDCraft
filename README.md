# CFDCraft

CFDCraft is an open-source workbench that puts the whole computational fluid dynamics chain in one coherent program: geometry in, meshing, case setup, solving, results, and many scenarios of the same study. It is written in Rust, built clean room from public documentation and its own probes, and made to be driven by people and by agents.

It is aimed at engineers whose core work is not CFD, such as building services and mechanical engineers, product designers and students. Commercial suites cost too much for them, and writing OpenFOAM dictionaries by hand is too hard.

The core idea: a non-specialist can take real geometry to a trustworthy, reproducible flow and heat result, and manage many scenarios of it, without a maze of tools or a folder of dictionaries.

## Status

CFDCraft is at the research stage. It is a command line and an agent interface today, not a finished application, and it has no desktop app or 3D viewport yet. Expect changes to file formats and commands.

What is built and tested in this repository:

- A solver-neutral study model with units, validated before anything runs, with plain-language diagnostics.
- A case writer for OpenFOAM (the OpenCFD v2512 line first, the Foundation v14 line second) that refuses code-executing entries, and a check that every value of the study reaches the written case.
- A runner for a solver in a container, locally or on a host you name, that judges success from solver evidence and not from exit codes.
- `.cfdcraft` study documents with crash-safe saves, schema migration, relative links, a content-addressed store for results, and pack and unpack.
- Input-addressed stage keys, staleness with reasons, immutable runs with provenance, and rerun-and-compare.
- Variants of variants, readable diffs, parameter sweeps checked before solving, pinning, notes and explicit pruning.
- A job queue with background running, live monitors, graceful stop and continue, and an honest convergence assessment.
- Five benchmark templates checked against published references, and a mesh independence study.
- A backend registry with parity tables, and OpenFOAM case import.
- A command line and an MCP server that offer the same typed operations.

What is not verified, and what to keep in mind:

- Verification so far is unit and acceptance tests in CI on macOS, Linux and Windows, plus runs on one private compute host. Real solver runs of the benchmarks gave some passes and some failures against the published tolerances, and some runs that passed had not fully converged. The check reports the convergence state next to the verdict and does not hide it.
- Some options compile and run but have not been exercised at full length on a real host, for example a potential-flow start on every template and several turbulence models on the backward-facing step.
- The cylinder mesh independence study changes the time step together with the mesh, so it mixes the two.
- The lattice-Boltzmann backend is a preview. Its results carry a notice that they are previews and are not gated results. Backends other than the OpenFOAM lines are admitted to parity checks step by step, and `cfdcraft backend list` shows the admission state of each.
- Geometry import from CAD files, the desktop app, ducts, buildings, external flow, radiation and conjugate heat transfer, and reports are planned and not built.

## Quickstart

CFDCraft is built with the Rust toolchain pinned in `rust-toolchain.toml`.

```
cargo build --release -p cfdcraft-cli
```

The commands below run on local files and do not need a solver.

- `cfdcraft backend list` shows the solver backends, their admission state, their images and the preview label.
- `cfdcraft benchmark list` shows the benchmark templates.
- `cfdcraft benchmark create lid-cavity --out cavity.json` writes a complete study for a template.
- `cfdcraft compile cavity.json case-out` compiles the study into a case folder, and `cfdcraft compile cavity.json case-out --backend <id>` does so for a named backend.
- `cfdcraft export cavity.json case-run` writes a runnable case with an `Allrun` script and a README, and `cfdcraft check-case case-run` scans a case for entries that could run code.
- `cfdcraft save cavity.json cavity.cfdcraft` turns the study into a document, `cfdcraft keys cavity.json` prints the mesh key and the result key, and `cfdcraft status cavity.cfdcraft` says whether the stored mesh and result are up to date and why not.
- `cfdcraft scenario` makes, lists, compares, names and pins variants of a study, and `cfdcraft sweep` plans, creates and checks parameter sweeps.

Running a solver needs a container host and an SSH key that you configure. Copy `crates/cfdcraft-executor/tests/hosts.example.toml` to `hosts.toml`, edit it for your host, and pass your key with `--key`.

- `cfdcraft run cavity.json --key <key file>` compiles and runs a study, then judges the solver evidence. `--smoke` runs a few iterations only.
- `cfdcraft solve cavity.cfdcraft --key <key file>` runs what is out of date and records the run in the document.
- `cfdcraft queue add`, `queue drain` and `queue watch` queue jobs, run them in the background and watch them.
- `cfdcraft rerun` runs a recorded run again from its provenance and compares the two, and `cfdcraft compare` compares the key numbers of two runs.

Every command takes `--help`, and most can print one JSON report with `--json`.

## Agents

`cfdcraft mcp` serves the typed operations over MCP on standard input and output. There are no shell tools and no free-form dictionary tools: an agent creates, validates, runs and checks studies through typed operations only. The same operations back the command line.

## Backends

Solvers are reached through a registry. Each backend names its dialect, its pinned container image and its admission state, and a backend is admitted only by passing the parity gate against the benchmarks. The registry holds the OpenFOAM lines (OpenCFD v2512 and Foundation v14), SU2, code_saturne and a lattice-Boltzmann preview solver. `cfdcraft parity` prints the parity table of a benchmark, one column for each backend, and `cfdcraft import-case` reads an existing OpenFOAM case into a read-only copy with an inventory of what it holds. OpenFOAM, SU2 and code_saturne run in their own processes, so their copyleft licences stay outside the CFDCraft code.

## Reproducible results

Every mesh and result is keyed by exactly what made it: the study, the solver image digest, the dialect, the writer version and the settings. The same inputs reuse the stored result, a changed input marks only the stages after it as stale and says why, and a run is never overwritten. A recorded run can be run again from its provenance and compared with the original within a stated tolerance.

## Benchmarks and mesh independence

Five templates each give a complete study that is checked against a published reference: the lid-driven cavity (`lid-cavity`, Ghia et al.), the turbulent backward-facing step (`backward-step`, Driver and Seegmiller), the laminar backward-facing step (`backward-step-laminar`, Armaly et al.), the cylinder in cross-flow (`cylinder`, Schafer and Turek, case 2D-2) and the differentially heated cavity (`heated-cavity`, de Vahl Davis).

- `cfdcraft benchmark list`
- `cfdcraft benchmark create cylinder --out cylinder.json --mesh fine --time transient --init potential`
- `cfdcraft save cylinder.json cylinder.cfdcraft`
- `cfdcraft benchmark check cylinder.cfdcraft --run run-1`

`benchmark create` writes a study for a template to a new file. `--mesh` takes `coarse`, `medium` (the default) or `fine`, which differ by a factor of two in cells along each direction. `--turbulence` takes `laminar`, `k-omega-sst`, `realizable-ke` or `rng-k-epsilon`, `--time` takes `steady` or `transient`, and `--init` takes `uniform` or `potential`. Each template has its own defaults, and the list says which. `cfdcraft save` turns the study file into a document, and `cfdcraft solve` records the runs that a check reads. The heated cavity runs with Boussinesq buoyancy and does not take a potential-flow start.

`benchmark check` compares the stored result of a recorded run with the published reference its study selects and prints, for every metric, the computed value, the published range, the tolerance used and the verdict. A failed check exits with status 1. The check also says whether the run it judged had converged, by the convergence assessment recorded with the run: a run that is not converged, or that has no assessment, is flagged with a warning line before the table and with `convergence` in the report, and the verdict and the exit status stay those of the comparison. A study that is not the benchmark it is checked against is refused with what differs. A tolerance file can widen a tolerance by at most a factor of three and needs a justification, which the report names.

- `cfdcraft gci create cylinder.cfdcraft --sweep gci-cylinder --levels 0,1,2`
- `cfdcraft queue add cylinder.cfdcraft --sweep gci-cylinder`
- `cfdcraft gci report cylinder.cfdcraft --sweep gci-cylinder --quantity bench.cylinder.st`

`gci create` stores a sweep over the mesh level with one variant for each level, which the queue runs like any other sweep. `gci report` reads a key number from the run of each variant and prints the observed order of convergence and the grid convergence index of the finest mesh with a safety factor of 1.25, with a warning when the series oscillates, lies outside the asymptotic range, is time dependent or relies on wall functions.

The same operations are tools of the agent interface: `benchmark_list`, `benchmark_create`, `benchmark_check`, `gci_create` and `gci_report`.

## Clean room

CFDCraft is written from public documentation, published papers and its own probes on files it may open. It uses no decompiled vendor binaries, no non-public interfaces and no code from readers whose licence is incompatible. Third-party material and its licences are listed in `NOTICE` and `ATTRIBUTION.md`.

## Data policy

CFDCraft collects nothing: no accounts, no telemetry, no update check. See `DATA-POLICY.md` for what the program does with the network.

## Contributing and security

Questions and contributions: hello@milleleaf.com. Security reports: see `SECURITY.md`.

## Licence

Code: MIT OR Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Third-party material and its licences: `NOTICE` and `ATTRIBUTION.md`.
