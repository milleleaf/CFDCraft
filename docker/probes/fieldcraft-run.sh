#!/usr/bin/env bash
# Runs the lattice-Boltzmann backend's own golden command stream through the tool's stdio server and
# prints how the tool behaved.
#
#   IMAGE=<FieldCraft image>
#
# Recorded: the version, a hash of the schema of exactly the commands the stream uses (read from the
# tool's own command listing), whether the server exits when its input ends (the stream run is stopped
# by `timeout 300`), the number of reply lines, and the shape of the first reply and of the reply to
# the fluid solve. Shapes list key names and value types only, never values. The server may read and
# write only the case folder.

set -u
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

name=fieldcraft
image=${IMAGE:-}
golden=$PROBE_ROOT/crates/cfdcraft-backend-lbm/tests/golden/cavity-lbm

if [ -z "$image" ]; then
  echo "IMAGE is not set"
  echo "PROBE-DONE fieldcraft-unconfigured"
  exit 2
fi

golden_ready "$golden" commands.jsonl
case $? in
  0) ;;
  2)
    echo "PROBE-SKIPPED $name golden not reviewed"
    exit 0
    ;;
  *)
    echo "PROBE-SKIPPED $name golden not present"
    exit 0
    ;;
esac

probe_begin "$name"
print_image_facts "$image"
cp -R "$golden/." "$PROBE_OUT/"
mkdir -p "$PROBE_OUT/out"

run_container "$image" "$name-version" \
  'cd /w && fieldcraft-cli --version; fieldcraft-cli commands --json > commands-schema.json 2> commands-schema.err; echo "commands exit status $?"; wc -c < commands-schema.json'

python3 -I - "$PROBE_OUT/commands-schema.json" "$PROBE_OUT/commands.jsonl" > "$PROBE_LOGS/$name.schema" 2>&1 << 'PY'
import hashlib
import json
import sys

CAP = 16 * 1024 * 1024
schema_path, stream_path = sys.argv[1], sys.argv[2]


def used_commands(path):
    used = set()
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh.read(CAP).splitlines():
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if not isinstance(msg, dict) or msg.get("method") != "tools/call":
                continue
            params = msg.get("params") or {}
            args = params.get("arguments") or {}
            if params.get("name") == "execute" and args.get("command"):
                used.add(args["command"])
            if params.get("name") == "batch":
                for c in args.get("commands") or []:
                    if isinstance(c, dict) and c.get("command"):
                        used.add(c["command"])
    return sorted(used)


try:
    with open(schema_path, encoding="utf-8", errors="replace") as fh:
        schema = json.loads(fh.read(CAP))
except (OSError, ValueError) as err:
    print("SCHEMA-UNREADABLE", type(err).__name__)
    sys.exit(0)

top = schema
if isinstance(top, dict) and isinstance(top.get("commands"), (list, dict)):
    top = top["commands"]
if isinstance(top, list):
    table = {e.get("command") or e.get("name"): e for e in top if isinstance(e, dict)}
elif isinstance(top, dict):
    table = top
else:
    table = {}

print("SCHEMA-SHAPE", type(schema).__name__, "entries", len(table))
first = next(iter(table.values()), None)
if isinstance(first, dict):
    print("SCHEMA-ENTRY-KEYS", sorted(first.keys()))

combined = hashlib.sha256()
for cmd in used_commands(stream_path):
    entry = table.get(cmd)
    if entry is None:
        print("MISSING", cmd)
        continue
    text = json.dumps(entry, sort_keys=True, separators=(",", ":")).encode()
    print("HASH", cmd, hashlib.sha256(text).hexdigest())
    combined.update(text)
print("HASH-OF-USED-COMMANDS", combined.hexdigest())
PY
print_text "$name schema hashes of the commands the stream uses" "$PROBE_LOGS/$name.schema"

run_container "$image" "$name-stream" \
  'cd /w && s=$(date +%s); timeout 300 fieldcraft-cli mcp --headless --automation-read-root /w --automation-write-root /w/out < commands.jsonl > replies.jsonl 2> log.fieldcraft; st=$?; echo "server exit status $st"; echo "elapsed $(( $(date +%s) - s )) s"; if [ "$st" = 124 ]; then echo "EXIT-AT-END-OF-INPUT no, stopped by the timeout"; else echo "EXIT-AT-END-OF-INPUT yes"; fi'

echo "REPLY-LINES $(wc -l 2> /dev/null < "$PROBE_OUT/replies.jsonl" || echo 0)"
excerpt "$name log.fieldcraft" "$PROBE_OUT/log.fieldcraft" 10 10

python3 -I - "$PROBE_OUT/replies.jsonl" "$PROBE_OUT/commands.jsonl" > "$PROBE_LOGS/$name.replies" 2>&1 << 'PY'
import json
import sys

CAP = 16 * 1024 * 1024
replies_path, stream_path = sys.argv[1], sys.argv[2]


def skeleton(obj, depth=0):
    """Key names and value types only, never values."""
    if depth > 6:
        return "..."
    if isinstance(obj, dict):
        return {k: skeleton(v, depth + 1) for k, v in sorted(obj.items())}
    if isinstance(obj, list):
        return [skeleton(obj[0], depth + 1), "x%d" % len(obj)] if obj else []
    if isinstance(obj, str):
        text = obj.strip()
        if text[:1] in "{[":
            try:
                return {"json-text": skeleton(json.loads(text), depth + 1)}
            except ValueError:
                pass
        return "str"
    return type(obj).__name__


def has_solve(msg):
    params = (msg.get("params") or {}) if isinstance(msg, dict) else {}
    args = params.get("arguments") or {}
    if args.get("command") == "solve.fluid":
        return True
    return any(isinstance(c, dict) and c.get("command") == "solve.fluid" for c in args.get("commands") or [])


solve_ids = set()
try:
    with open(stream_path, encoding="utf-8", errors="replace") as fh:
        for line in fh.read(CAP).splitlines():
            try:
                msg = json.loads(line)
            except ValueError:
                continue
            if has_solve(msg):
                solve_ids.add(json.dumps(msg.get("id")))
except OSError:
    print("STREAM-UNREADABLE")

try:
    with open(replies_path, encoding="utf-8", errors="replace") as fh:
        lines = [ln for ln in fh.read(CAP).splitlines() if ln.strip()]
except OSError:
    lines = []

print("REPLIES", len(lines))
parsed = []
for ln in lines:
    try:
        parsed.append(json.loads(ln))
    except ValueError:
        parsed.append(None)
print("REPLIES-THAT-PARSE", sum(1 for p in parsed if p is not None))
if parsed and isinstance(parsed[0], dict):
    print("FIRST-REPLY-KEYS", sorted(parsed[0].keys()))
    print("FIRST-REPLY-SHAPE", json.dumps(skeleton(parsed[0]), sort_keys=True))
for p in parsed:
    if isinstance(p, dict) and json.dumps(p.get("id")) in solve_ids:
        print("SOLVE-FLUID-REPLY-SHAPE", json.dumps(skeleton(p), sort_keys=True))
if not solve_ids:
    print("NO-SOLVE-FLUID-CALL-IN-THE-STREAM")
PY
print_text "$name reply shapes" "$PROBE_LOGS/$name.replies"
# The replies themselves (capped at 200 KiB by print_text): the judge's real-output test reads them.
print_text "$name replies.jsonl" "$PROBE_OUT/replies.jsonl"

list_files "$PROBE_OUT" > "$PROBE_LOGS/$name.files"
print_text "$name files (name and size)" "$PROBE_LOGS/$name.files"

echo "PROBE-DONE $name"
