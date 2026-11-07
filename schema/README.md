# CFDCraft study file schemas

These JSON Schema files describe the JSON inside a `.cfdcraft` study file, for people writing tools that create or read studies without using CFDCraft's own code. They are generated from the types CFDCraft itself reads, and a test fails if a file here differs from what those types generate.

| File | Describes |
|---|---|
| `cfdcraft-manifest-v4.json` | `manifest.json` of a file at schema 4: the format name, the schema number and the program that wrote the file |
| `cfdcraft-study-v1.json` | `study.json`: the study itself, which carries its own number (1) |
| `cfdcraft-artifacts-v4.json` | `artifacts.json` of a file at schema 4: stored results, linked files, the runs (including stopped and continued runs and the verdict on each), the variants, the sweeps and the pruned results |
| `cfdcraft-queue-v1.json` | `queue.json`, the list of jobs kept beside a study file; it carries its own number (1) |
| `cfdcraft-manifest-v3.json` | `manifest.json` of a file written at schema 3; frozen (see Versions) |
| `cfdcraft-artifacts-v3.json` | `artifacts.json` of a file written at schema 3; frozen (see Versions) |
| `cfdcraft-manifest-v2.json` | `manifest.json` of a file written at schema 2; frozen (see Versions) |
| `cfdcraft-artifacts-v2.json` | `artifacts.json` of a file written at schema 2; frozen (see Versions) |
| `cfdcraft-manifest-v1.json` | `manifest.json` of a file written at schema 1; frozen (see Versions) |
| `cfdcraft-artifacts-v1.json` | `artifacts.json` of a file written at schema 1; frozen (see Versions) |

Use the manifest and artifact-index files that match the schema number of the file you are checking. The study file is the same for all of them. The queue file is not a member of the study file: it lives beside it and has its own file.

## The file

A `.cfdcraft` file is a ZIP archive. All JSON is UTF-8. A reader looks members up by name and ignores every other entry.

| Member | Required | Content |
|---|---|---|
| `manifest.json` | yes | read first; the file is judged by this member alone before anything else is read |
| `study.json` | yes | the study |
| `artifacts.json` | no | stored results, linked files, runs, variants, sweeps and pruned results; a file without it is read as having none |
| `thumbnail.png` | no | a PNG picture of the study, stored without compression |

CFDCraft writes the members in the order above, with the manifest first, and a tool should do the same. Reading does not depend on the order, but a name that appears twice makes the file unreadable.

## Versions

Two numbers are in play, and they are not the same thing.

The number of the file is `schema_version` in the manifest. One integer covers the whole file (currently 4). The manifest and artifact-index schema files carry it in their names (`-v4` is file schema 4), and the manifest schema allows exactly its own number.

The study carries its own number in `schema_version` inside `study.json` (currently 1). It changes only when the study changes in a way that makes an earlier study unreadable, so a file at schema 4 holds a study at number 1, and `cfdcraft-study-v1.json` describes it.

The study file has been extended twice without a new number. It gained the optional member `raw_overrides` (see Raw overrides), and then the optional members `probes`, `lines`, `surfaces` and `settle` inside `monitors` (see Monitors). Every study that was accepted before is still accepted by `cfdcraft-study-v1.json`, and a study that leaves the members out is unchanged, including its hash. The number stays 1 on purpose: raising it would change the canonical hash of every study.

A reader refuses a file whose number is newer than it knows, deciding from the manifest alone, before it reads the study. Older files are brought forward to the current number when they are opened, and the reader reports what changed.

A schema file is never edited once its number has been superseded. A change to the format ships as a new version, with new files next to the old ones. The files for schemas 1, 2 and 3 stay in this folder, as they were when the next schema was introduced, for files written at those numbers. A test holds a copy of each and a recorded length and checksum, so an edit to one of them fails the build.

One edit to a schema 1 file is on record, and it came before the freeze. The artifact-index file for schema 1 got the link `id` pattern (`^[a-z0-9][a-z0-9-]{0,39}$`) and its description about an hour and a quarter after the first schema files were written (the commits 60c1f9d and 844169c), before any release or tag, in a fix that made the schema state a rule the reader already enforced. No file the earlier text accepted and CFDCraft opened is refused by the later one, because the reader refused a link `id` outside that pattern already. The frozen schema 1 text is the text with the pattern.

## Validating

Use any JSON Schema validator that supports draft 2020-12, with the file that matches the member and the version. The schemas contain no references to anything outside themselves, so a validator never needs to fetch anything. Their `$id` values are URNs (for example `urn:cfdcraft:schema:study:1`), not web addresses.

A file written at schema 1 validates against the schema 1 manifest and artifact-index files with the study file. A file at schema 2 validates against the schema 2 files, a file at schema 3 against the schema 3 files and a file at schema 4 against the schema 4 files, in every case with the same study file. A `queue.json` validates against `cfdcraft-queue-v1.json`.

Unknown keys are rejected in the manifest, the study, the artifact index, links, stored results, run records, variants, sweeps and raw overrides (these records are closed). The labels under `boundary_conditions` and the names under `key_numbers` and a result recipe's `boundaries` are free-form keys. The recipe, tool, comparison and tolerance records inside a run record are not closed in the schema, and the reader ignores an unknown key there too.

## The study

`study.json` holds these top-level keys, all required: `schema_version`, `domain`, `fluid`, `boundary_conditions`, `mesh`, `solver` and `monitors`.

- `domain` has three lengths, `length_x`, `length_y` and `length_z`, and optionally `open` (true when the box has inlet or outlet faces; false when omitted).
- `fluid` has `kinematic_viscosity`.
- `boundary_conditions` is an object keyed by a label of your choice. Each entry has `faces` (a list of `XMin`, `XMax`, `YMin`, `YMax`), `kind` (one of `VelocityInlet`, `PressureOutlet`, `NoSlipWall`, `SlipWall`, `MovingWall`, `Symmetry`) and, for a velocity inlet and a moving wall, `velocity`.
- `mesh` has the whole numbers `cells_x`, `cells_y` and `cells_z`.
- `solver` has `solver` (`SimpleLaminar`), `end_time` and `write_interval`, and optionally `convergence` with `p` and `U`.
- `monitors` has `centreline_u_sample_count` and, optionally, `probes`, `lines`, `surfaces` and `settle`; see Monitors.
- `raw_overrides` is optional; see Raw overrides.

A quantity is an object with a number `value` and a `unit`. The units the schema lists are `m`, `mm`, `m/s`, `m2/s`, `Pa`, `K`, `s`, `1` and `rpm`. The reader also accepts `m²/s` as another way to write `m2/s`; the schema does not list it, so a document that uses it is rejected by the schema and accepted by CFDCraft. Write `m2/s`.

## What the schema does not check

The schemas check the shape of each member: keys, types, allowed words, the schema number and the hash format. They do not check anything that needs more than one field to decide. In particular they do not check that boundary labels use the allowed characters, that every face of the box is covered, or that an open domain has an inlet and an outlet, or that no two links share an `id`. Run `cfdcraft validate` on a study to get those checks.

More things are beyond the schema, and CFDCraft checks them when it opens or saves a file:

- The key in a run record matches its recipe: every key is the BLAKE3 hash of the canonical JSON of the recipe next to it. The schema accepts any 64 lower-case hexadecimal characters.
- Every stored result a run record names is listed in `blobs`, with the same size.
- Run ids are unique and in order (`run-1`, `run-2` and so on), and a rerun names an earlier run.
- A variant's parent is `base` or a variant listed earlier in `scenarios`, so a variant is never listed before its parent. Ids are plain names (the same rule as a link id), `base` holds no parent, overrides or origin, a chain of parents is at most 16 levels deep, a file holds at most 1000 variants, a variant at most 200 overrides, a name is 1 to 80 characters and notes at most 2000, and each setting is a well-formed pointer.
- A sweep's `base` is `base` or a listed variant, sweep ids are unique, a sweep has at most 8 parameters, at most 1000 values for each parameter and at most 200 points, and a variant's `origin` names a listed sweep and a point it has. A run's `scenario` names a listed variant.
- `pruned` is sorted, has no repeat and lists only hashes that `blobs` lists.
- A stopped run holds a `checkpoint` and no `result`, and a run that went to its end holds no `checkpoint`. A run that continues another names an earlier run. A process count is at least 1.
- In a queue, at most one job is active, an active job has a run id and a queued job has none, a finished or stopped job names its run and a failed or interrupted job has a code.

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
- `provenance` is what the run stood on: the `geometry_hash`, the `mesh_key` and `mesh_hash` of the mesh, the `result_key` when a result was recorded, the `settings` (the study as it was when the case ran), the `tool` identity and the `host` label. The tool identity holds the `image_digest` of the solver image, the `dialect` the case is written in, the `sidecar_lock` (null when no geometry sidecar was used) and the `writer_version` of CFDCraft (its version, then `+` and a fingerprint of the source of the case writer, so a writer that emits a different case has a different identity even under one version number). The `host` is a label only and can be cleared when a file is shared.
- `checks` lists the named checks the run was held to, each with `name`, `passed` and a short `detail`.
- `key_numbers` holds the named numbers a rerun is compared on, such as iteration counts, residuals and centreline samples.
- `comparison` is present on a rerun. It gives the verdict (`reproduced`, `within_tolerance` or `differs`), the tolerance used (`abs` and `rel`), how many quantities were compared, the largest difference, and the quantities that are not identical.

Each record may carry a `scenario`, the id of the variant it was made for. Without it the run is of the study itself. A rerun carries the same tag as the run it repeats.

The mesh recipe and the result recipe each have an optional `raw` member that holds the raw overrides the recipe depends on (see Raw overrides). Both are left out when there are none, so a recipe without raw overrides, and its key, are what they were.

When an older file is opened it is brought forward to the current number and the reader says what changed, one note for each step. A schema 1 file opens with three notes (1 to 2: an empty history; 2 to 3: room for variants, sweeps and pruned results; 3 to 4: room for stopped and continued runs and the verdict on a result), a schema 2 file with two and a schema 3 file with one. Nothing else about the file changes.

Schema 4 adds five optional members to a run record, and a run that has none of them is an ordinary complete run:

- `outcome` is the word `complete` or an object `{"stopped": {"at_time": "<solver time>"}}`. A run that was stopped on request has the second form, a `checkpoint` and no `result`.
- `checkpoint` is the latest written time of a stopped run: `time` (plain decimal text), the `blob` hash and the `size` of the stored data. A later run can start from it.
- `continued_from` names the earlier run (`run`) and the solver time (`time`) that a continued run started from.
- `verdict` is the judgement made when the run was recorded on whether it had converged: a `state` (`converged`, `not_converged` or `not_judged`), the `reasons`, the numbers of every monitored quantity (`last`, `mean`, `spread`, `drift`, `status` and the number of `samples`) and the `mass` and `energy` balances. A run counts as converged only when every monitored quantity has settled and the balances hold; the residuals alone never decide it.
- `np` is the number of processes the run used.

A result recipe's `monitors` member now lists the probes, lines and surface integrals the case wrote, in order and in metres, when there are any.

## Variants and sweeps

The `scenarios` entry of `artifacts.json` (schema 3) lists the variants of the study. It is optional, and a file without it, or with an empty list, has none. A variant stores only what it changes, never a copy of its parent.

- `id` is a plain name. The id `base` is reserved for the study itself: a record with that id carries only a name, a pin and notes for the study, and has no parent, overrides or origin.
- `parent` is the variant this one descends from, or `base`. A variant is listed after its parent. The effective study of a variant is the study of the file with the overrides of every ancestor applied first, root to leaf, and its own last.
- `overrides` is a list. Each override has an `op`. `set` puts `value` at `setting` (replacing the member, or adding it when its parent exists) and `remove` deletes the member at `setting`. A `setting` is a JSON Pointer (RFC 6901) into the study written as JSON, such as `/mesh/cells_x`. Only object members are addressed; a list such as the faces of a boundary is replaced whole. The result must read back as a valid study.
- `name` is a short label, `pinned` marks a variant a person wants to keep, and `notes` is free text.
- `origin` names the sweep (`sweep`) and the point (`index`, counted from 0) that made the variant, when a sweep made it.

The `sweeps` entry lists parameter sweeps. A sweep has an `id`, a `base` (a variant, or `base`) and a list of `parameters`. A parameter has a `name`, a `target` and `values`.

- A target has a `kind`. `quantity` sets a number with a `unit` at a `setting`, `count` sets a whole number, `number` sets a plain number, and `direction` turns the vector `base_x`, `base_y` by an angle and sets its two components at `setting_x` and `setting_y` in `unit`. The values of a direction parameter are angles in degrees, counter-clockwise.
- The values have a `mode`. `list` gives `items`, `range` gives `start`, `stop` and `step` (the stop is included when the steps land on it), and `steps` gives `start`, `stop` and a `count` of values spread evenly.
- A sweep expands into one point for each combination of the values of its parameters, the first parameter varying slowest. A point is a variant named `<sweep>-<number>`, where the number is the position of the point counted from 1, with three digits.

The `pruned` entry lists the hashes of stored results that were removed from the store on purpose. Every such hash is also listed in `blobs`, and the run records that named the result stay, with their key numbers. Storing the same result again takes its hash off the list.

## Monitors

The optional members of `monitors` say what to watch while the solver runs. Each is an object keyed by a label of your choice (the same naming rule as a boundary label), and the label is for people only: it never reaches a solver case.

- `probes`: each has `x` and `y` (quantities, a point in the plane of the case) and `field` (`velocity` or `pressure`). At most 16.
- `lines`: each has the ends `x0`, `y0`, `x1`, `y1` (quantities), `samples` (2 to 2000 points, ends included) and `field`. At most 8.
- `surfaces`: each has the `boundary` (the label of a boundary of the study) and the `quantity` (`flux`, `pressure_average` or `pressure_integral`). At most 8.
- `settle`: optional tolerances for judging a monitored quantity settled: `tail_fraction`, `tail_floor`, `spread_rel` and `drift_rel`. A member left out uses the built-in value. They judge a run and do not change what is run.

A study that lists no probe, line or surface is watched at five default velocity points. The schema checks the shape; the ranges, the labels and the positions inside the domain are checked by CFDCraft when it validates a study.

## The queue file

`queue.json` sits in the store folder beside a study file and lists the jobs to run, in the order they start. It carries its own `schema_version` (currently 1) and is described by `cfdcraft-queue-v1.json`. A job has an `id` (`q-` and a number), a `kind` (`solve`, or `{"continue": {"run": "<stopped run>"}}`), a `state` and, as it goes, a `run_id`, the `run` it recorded, a `code` when it failed, a pending `stop` and a display-only `progress`. `next_number` is the number the next job gets and is never lowered. A reader refuses a queue file whose number is newer than it knows.

## Raw overrides

A raw override sets one solver dictionary entry that the study does not model. It is meant for experts, and it shows wherever the study is compared. `study.json` holds them in the optional member `raw_overrides`, an object keyed by a label that follows the same rule as a boundary label. Each override has four members:

- `backend` is `esi` or `foundation`, the solver backend the override is written for. A backend refuses an override that is tagged for the other one.
- `file` is the case file: `system/`, `constant/` or `0/`, then one or two plain names of letters, digits, `_`, `.` and `-`, with no `..`.
- `entry` is the entry inside the file: one to six names joined by `/`, each a letter followed by up to 63 letters, digits or underscores, such as `relaxationFactors/equations/U`.
- `value` is the text that goes after the entry name: 1 to 200 printable ASCII characters, none of `;`, `{`, `}`, `#`, `$`, `"` or a backslash.

A study holds at most 50. An override in `system/blockMeshDict`, `system/snappyHexMeshDict`, `system/meshQualityDict`, `system/surfaceFeaturesDict` or below `constant/polyMesh/` shapes the mesh, so it is part of the mesh recipe and a change to it needs a new mesh. Every override is part of the result recipe, by its label. The schema checks the shape of each member. The rules on the file, entry and value text, the limit of 50 and the backend are checked by CFDCraft when it validates a study, and a study that breaks them passes the schema.

## Regenerating the files

When a Rust type behind a schema changes, the checked-in files must be regenerated and the change reviewed:

```
CFDCRAFT_UPDATE_SCHEMA=1 cargo test -p cfdcraft-schema --test schema_tests
```

Without the variable the same test only compares, and fails with the file name when a file differs. If the format itself changes, raise the schema number and publish new files instead of editing these.
