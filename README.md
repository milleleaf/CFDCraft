# CFDCraft

Computational fluid dynamics; an open-source, clean-room take on the Fluid Dynamics Analysis workflow in pure Rust, built to be driven by agents.

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

## Licence

Code: MIT OR Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Third-party material and its licences: `NOTICE` and `ATTRIBUTION.md`.
