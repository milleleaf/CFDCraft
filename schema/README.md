# CFDCraft study file schemas

These JSON Schema files describe the JSON inside a `.cfdcraft` study file, for people writing tools that create or read studies without using CFDCraft's own code. They are generated from the types CFDCraft itself reads, and a test fails if a file here differs from what those types generate.

| File | Describes |
|---|---|
| `cfdcraft-manifest-v1.json` | `manifest.json`: the format name, the schema number and the program that wrote the file |
| `cfdcraft-study-v1.json` | `study.json`: the study itself |
| `cfdcraft-artifacts-v1.json` | `artifacts.json`: stored results and linked files |

## The file

A `.cfdcraft` file is a ZIP archive. All JSON is UTF-8. A reader looks members up by name and ignores every other entry.

| Member | Required | Content |
|---|---|---|
| `manifest.json` | yes | read first; the file is judged by this member alone before anything else is read |
| `study.json` | yes | the study |
| `artifacts.json` | no | stored results and linked files; a file without it is read as having none |
| `thumbnail.png` | no | a PNG picture of the study, stored without compression |

CFDCraft writes the members in the order above, with the manifest first, and a tool should do the same. Reading does not depend on the order, but a name that appears twice makes the file unreadable.

## Versions

One integer, `schema_version` in the manifest, covers the whole file. The schema file names carry it (`-v1` is schema number 1). The manifest and the study both hold the number, and each schema file allows exactly its own number.

A reader refuses a file whose number is newer than it knows, deciding from the manifest alone, before it reads the study. Older files are brought forward to the current number when they are opened, and the reader reports what changed.

A published schema file is never edited. A change to the format ships as a new version, with new files next to the old ones.

## Validating

Use any JSON Schema validator that supports draft 2020-12, with the file that matches the member and the version. The schemas contain no references to anything outside themselves, so a validator never needs to fetch anything. Their `$id` values are URNs (for example `urn:cfdcraft:schema:study:1`), not web addresses.

Unknown keys are rejected: every record in a schema is closed. The labels under `boundary_conditions` are the only free-form keys.

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

The schemas check the shape of each member: keys, types, allowed words, the schema number and the hash format. They do not check anything that needs more than one field to decide. In particular they do not check that boundary labels use the allowed characters, that every face of the box is covered, or that an open domain has an inlet and an outlet. Run `cfdcraft validate` on a study to get those checks.

## Limits

A reader enforces these sizes on the uncompressed members: the manifest at most 64 KiB, the study and the artifact index at most 32 MiB each, the thumbnail at most 8 MiB. A file that lists the same entry name twice is refused. Entries with other names are ignored, never extracted and never used as paths.

## Results and linked files

Results do not live inside the ZIP. They are kept in a store folder beside the document, in files named by the BLAKE3 hash of their content. `artifacts.json` lists them under `blobs`, each with its `hash` (64 lower-case hexadecimal characters), its `size` in bytes and its `kind` (`surface`, `mesh` or `result`).

Files a study refers to but does not contain are listed under `links`. Each link has an `id`, a `path` relative to the folder that holds the document, the file `name` and the BLAKE3 `hash` of the content as recorded when the link was made.

## Regenerating the files

When a Rust type behind a schema changes, the checked-in files must be regenerated and the change reviewed:

```
CFDCRAFT_UPDATE_SCHEMA=1 cargo test -p cfdcraft-schema --test schema_tests
```

Without the variable the same test only compares, and fails with the file name when a file differs. If the format itself changes, raise the schema number and publish new files instead of editing these.
