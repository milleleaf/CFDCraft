# Exported case

This directory is a complete OpenFOAM case with an `Allrun` script. It runs without CFDCraft.

Study hash: `ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0`

## Image

The case is run in a pinned image, referenced by digest so the solver build cannot change underneath it.

```
docker pull opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319
```

## Run

From this directory:

```
docker run --rm --user "$(id -u):$(id -g)" --network none -v "$PWD":/w opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319 bash -lc 'cd /w && sh ./Allrun'
```

The `cd /w` is required: the image entrypoint resets the working directory, so without it the commands run somewhere else.

CFDCraft's own runs add `--cpus 2 --memory 8g --memory-swap 8g --pids-limit 256`; similar limits are sensible on a shared machine.

## Judge the result by the logs, not the exit status

A run is good only when each of the three logs (`log.blockMesh`, `log.checkMesh`, `log.simpleFoam`) ends with `End`, `log.checkMesh` contains `Mesh OK`, no log reports a fatal error or a floating-point exception, and the directory named for the end time in `system/controlDict` holds non-empty `U` and `p`. The start-up banner that says floating-point trapping is enabled is normal.

The exit status of `Allrun` is a convenience only.

## Other CPU architectures

The image is a multi-architecture index, so results on a different CPU architecture can differ in the last printed digits. Judge them against a stated tolerance, not byte equality.

## Trade marks

CFDCraft is not approved or endorsed by OpenCFD Limited. OPENFOAM® and OpenCFD® are registered trade marks of OpenCFD Limited.
