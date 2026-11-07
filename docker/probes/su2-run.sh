#!/usr/bin/env bash
# Runs SU2_CFD on the SU2 backend's own golden cavity and prints what it produced.
#
#   IMAGE=<SU2 image>
#
# It records the facts the backend's output markers stand on: the closing banner, the history file's
# header and last row, every output file with its size, and the help line of the thread flag. The
# solver is stopped by a hard `timeout 600` inside the container.

set -u
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

name=su2
image=${IMAGE:-}
golden=$PROBE_ROOT/crates/cfdcraft-backend-su2/tests/golden/cavity-su2

if [ -z "$image" ]; then
  echo "IMAGE is not set"
  echo "PROBE-DONE su2-unconfigured"
  exit 2
fi

golden_ready "$golden" config.cfg mesh.su2
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

run_container "$image" "$name-help" 'SU2_CFD -h 2>&1 | grep -i -E "thread" | head -n 5; command -v SU2_CFD'
run_container "$image" "$name-run" \
  'cd /w && timeout 600 SU2_CFD -t 3 config.cfg > log.SU2 2>&1; echo "solver exit status $?"'

excerpt "$name log.SU2" "$PROBE_OUT/log.SU2" 15 60

for h in "$PROBE_OUT"/history*; do
  [ -f "$h" ] || continue
  echo "HISTORY $(basename "$h") rows $(wc -l < "$h")"
  { head -n 1 "$h"; tail -n 1 "$h"; } > "$PROBE_LOGS/$name.history"
  print_text "$name $(basename "$h") header line and last row" "$PROBE_LOGS/$name.history"
  # The whole history (capped at 200 KiB by print_text) is what the judge's real-output test reads.
  print_text "$name $(basename "$h") whole file" "$h"
done

for v in "$PROBE_OUT"/flow.csv "$PROBE_OUT"/flow*.csv; do
  [ -f "$v" ] || continue
  excerpt "$name $(basename "$v") (first and last rows)" "$v" 4 2
  break
done

list_files "$PROBE_OUT" > "$PROBE_LOGS/$name.files"
print_text "$name output files (name and size)" "$PROBE_LOGS/$name.files"

echo "PROBE-DONE $name"
