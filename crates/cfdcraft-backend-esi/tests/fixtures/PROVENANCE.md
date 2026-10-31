# OpenFOAM regression log provenance

The three `of14-*` files are verbatim captured output from cfdcraft's own 2026-10-25 smoke run on a remote Linux host, copied read-only on 2026-10-28.

`of14-windaround-buildings.log` is `log.surfaceFeatures` (2,321 bytes), `of14-snappyHexMesh.log` is `log.snappyHexMesh` (1,786 bytes), and `of14-foamRun.log` is `log.foamRun` (319,330 bytes). Surface extraction crashed with a floating-point exception, meshing reported a fatal missing-feature error, and the later solver still ended normally on the wrong mesh. These process outputs are regression evidence; they are not OpenFOAM source code.

`cavity-good.log` is an explicitly synthetic minimal passing log used to isolate individual evidence rules. Live solver behavior is verified separately by a real end-to-end run.

The `v2512-blockMesh.log` and `v2512-checkMesh.log` fixtures are verbatim output from the first real CLI cavity run on 2026-10-28, run id `cfdcraft-cavity-40359-1793170628247501000`. `v2512-simpleFoam-final-iteration.log` preserves that solver log's original header and final iteration block verbatim, with intermediate iterations omitted. The run reached iteration 2000 but had p/U initial residuals above 1e-6; this is a negative convergence fixture. These logs reproduce the benign v2512 `Floating point exception trapping enabled (FOAM_SIGFPE)` banner and total-volume sentence punctuation that the first built parser misclassified.
