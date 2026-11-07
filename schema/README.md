# CFDCraft study file schemas

These JSON Schema files describe the JSON inside a `.cfdcraft` study file, for people writing tools that create or read studies without using CFDCraft's own code. They are generated from the types CFDCraft itself reads, and a test fails if a file here differs from what those types generate.

| File | Describes |
|---|---|
| `cfdcraft-manifest-v2.json` | `manifest.json` of a file at schema 2: the format name, the schema number and the program that wrote the file |
| `cfdcraft-study-v1.json` | `study.json`: the study itself, which carries its own number (1) |
| `cfdcraft-artifacts-v2.json` | `artifacts.json` of a file at schema 2: stored results, linked files and the runs |
| `cfdcraft-manifest-v1.json` | `manifest.json` of a file written at schema 1; kept as it was published |
| `cfdcraft-artifacts-v1.json` | `artifacts.json` of a file written at schema 1; kept as it was published |

Use the manifest and artifact-index files that match the schema number of the file you are checking. The study file is the same for both.

## The file

A `.cfdcraft` file is a ZIP archive. All JSON is UTF-8. A reader looks members up by name and ignores every other entry.

| Member | Required | Content |
|---|---|---|
| `manifest.json` | yes | read first; the file is judged by this member alone before anything else is read |
| `study.json` | yes | the study |
| `artifacts.json` | no | stored results, linked files and runs; a file without it is read as having none |
| `thumbnail.png` | no | a PNG picture of the study, stored without compression |

CFDCraft writes the members in the order above, with the manifest first, and a tool should do the same. Reading does not depend on the order, but a name that appears twice makes the file unreadable.

## Versions

Two numbers are in play, and they are not the same thing.

The number of the file is `schema_version` in the manifest. One integer covers the whole file (currently 2). The manifest and artifact-index schema files carry it in their names (`-v2` is file schema 2), and the manifest schema allows exactly its own number.

The study carries its own number in `schema_version` inside `study.json` (currently 1). It changes only when the study itself changes, so a file at schema 2 holds a study at number 1, and `cfdcraft-study-v1.json` describes it.

A reader refuses a file whose number is newer than it knows, deciding from the manifest alone, before it reads the study. Older files are brought forward to the current number when they are opened, and the reader reports what changed.

A published schema file is never edited. A change to the format ships as a new version, with new files next to the old ones. The files for schema 1 stay in this folder for files written at that number.

## Validating

Use any JSON Schema validator that supports draft 2020-12, with the file that matches the member and the version. The schemas contain no references to anything outside themselves, so a validator never needs to fetch anything. Their `$id` values are URNs (for example `urn:cfdcraft:schema:study:1`), not web addresses.

A file written at schema 1 validates against the schema 1 manifest and artifact-index files with the study file. A file at schema 2 validates against the schema 2 files with the same study file.

Unknown keys are rejected in the manifest, the study, the artifact index, links, stored results and run records (these records are closed). The labels under `boundary_conditions` and the names under `key_numbers` and a result recipe's `boundaries` are free-form keys. The recipe, tool, comparison and tolerance records inside a run record are not closed in the schema, and the reader ignores an unknown key there too.

## The study

`study.json` holds these top-level keys, all required: `schema_version`, `domain`, `fluid`, `boundary_conditions`, `mesh`, `solver` and `monitors`.

- `domain` has three lengths, `length_x`, `length_y` and `length_z`, and optionally `open` (true when the box has inlet or outlet faces; false when omitted).
- `fluid` has `kinematic_viscosity`.
- `boundary_conditions` is an object keyed by a label of your choice. Each entry has `faces` (a list of `XMin`, `XMax`, `YMin`, `YMax`), `kind` (one of `VelocityInlet`, `PressureOutlet`, `NoSlipWall`, `SlipWall`, `MovingWall`, `Symmetry`) and, for a velocity inlet and a moving wall, `velocity`.
- `mesh` has the whole numbers `cells_x`, `cells_y` and `cells_z`.
- `solver` has `solver` (`SimpleLaminar`), `end_time` and `write_interval`, and optionally `convergence` with `p` and `U`.
- `monitors` has `centreline_u_sample_count`.

A quantity is an object with a number `value` and a `unit`. The units the schema lists are `m`, `mm`, `m/s`, `m2/s`, `Pa`, `K`, `s`, `1` and `rpm`. The reader also accepts `m²/s` as another way to write `m2/s`; the schema does not list it, so a document that uses it is rejected by the schema and accepted by CFDCraft. Write `m2/s`.

## What the schema does not check

The schemas check the shape of each member: keys, types, allowed words, the schema number and the hash format. They do not check anything that needs more than one field to decide. In particular they do not check that boundary labels use the allowed characters, that every face of the box is covered, or that an open domain has an inlet and an outlet, or that no two links share an `id`. Run `cfdcraft validate` on a study to get those checks.

Three more things are beyond the schema, and CFDCraft checks them when it opens or saves a file:

- The key in a run record matches its recipe: every key is the BLAKE3 hash of the canonical JSON of the recipe next to it. The schema accepts any 64 lower-case hexadecimal characters.
- Every stored result a run record names is listed in `blobs`, with the same size.
- Run ids are unique and in order (`run-1`, `run-2` and so on), and a rerun names an earlier run.

A file where any of these fails passes the schema and is refused by CFDCraft.

## Limits

A reader enforces these sizes on the uncompressed members: the manifest at most 64 KiB, the study and the artifact index at most 32 MiB each, the thumbnail at most 8 MiB. A file that lists the same entry name twice is refused. Entries with other names are ignored, never extracted and never used as paths.

## Results and linked files

Results do not live inside the ZIP. They are kept in a store folder beside the document, in files named by the BLAKE3 hash of their content. `artifacts.json` lists them under `blobs`, each with its `hash` (64 lower-case hexadecimal characters), its `size` in bytes and its `kind` (`surface`, `mesh` or `result`).

Files a study refers to but does not contain are listed under `links`. Each link has an `id` (1 to 40 characters, each a lower-case letter `a-z`, a digit or a hyphen, the first not a hyphen; the schema carries this as a `pattern`), a `path` relative to the folder that holds the document, the file `name` and the BLAKE3 `hash` of the content as recorded when the link was made. The `id` of every link must be different from the others. JSON Schema cannot express that a field is unique among array items, so the schema accepts a file that repeats an `id` and CFDCraft refuses to open it. Give each link its own `id`.

## Runs

The `runs` entry of `artifacts.json` (schema 2) is the history of what was made from the study. It is a list, oldest first, and it only grows: a record is never edited after it is written. It is optional, and a file without it, or with an empty list, is valid. Run records are written by CFDCraft. A tool that authors a study file should leave `runs` out or empty.

Every key in a record is the BLAKE3 hash of the canonical JSON (RFC 8785) of its recipe. CFDCraft checks that when it reads a file, so a record that was edited by hand, or written with made-up keys, is refused.

A run record has these parts:

- `id` is `run-` and a number, counted from the number of records before it. `rerun_of`, when present, names the earlier run that this one repeats.
- `mesh` and `result` are the outputs the run recorded. Either can be absent: a run records only the stages it made, and a rerun records only a result. Each output has its `key`, the `recipe` the key comes from, the `blob` hash of the stored data and its `size` in bytes. The mesh recipe holds the tool identity, the geometry in metres, the patch layout and the cell counts. The result recipe holds the tool identity, the key of the mesh it stands on, the fluid, the boundaries by label, the solver settings and the monitors. All numbers in a recipe are in SI units.
- `provenance` is what the run stood on: the `geometry_hash`, the `mesh_key` and `mesh_hash` of the mesh, the `result_key` when a result was recorded, the `settings` (the study as it was when the case ran), the `tool` identity and the `host` label. The tool identity holds the `image_digest` of the solver image, the `dialect` the case is written in, the `sidecar_lock` (null when no geometry sidecar was used) and the `writer_version` of CFDCraft. The `host` is a label only and can be cleared when a file is shared.
- `checks` lists the named checks the run was held to, each with `name`, `passed` and a short `detail`.
- `key_numbers` holds the named numbers a rerun is compared on, such as iteration counts, residuals and centreline samples.
- `comparison` is present on a rerun. It gives the verdict (`reproduced`, `within_tolerance` or `differs`), the tolerance used (`abs` and `rel`), how many quantities were compared, the largest difference, and the quantities that are not identical.

When an older file is opened, a schema 1 file is brought forward to schema 2 with an empty history, and the reader says so in one note. Nothing else about the file changes.

## Regenerating the files

When a Rust type behind a schema changes, the checked-in files must be regenerated and the change reviewed:

```
CFDCRAFT_UPDATE_SCHEMA=1 cargo test -p cfdcraft-schema --test schema_tests
```

Without the variable the same test only compares, and fails with the file name when a file differs. If the format itself changes, raise the schema number and publish new files instead of editing these.
