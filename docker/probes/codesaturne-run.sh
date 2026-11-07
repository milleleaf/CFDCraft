#!/usr/bin/env bash
# Runs code_saturne on the backend's own golden cavity: the initialisation stage, the full run and
# the mesh quality check, and prints what each left behind.
#
#   IMAGE=<code_saturne image>
#
# The case folder is the golden one (DATA/setup.xml and the mesh) with empty SRC and RESU folders
# added, so nothing the case folder lacks is supplied by the probe except those two empty folders.
# Printed: the log tails, whether any error file exists, the name and first lines of the residual
# history and of every monitoring file, and the quality check output.

set -u
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

name=codesaturne
image=${IMAGE:-}
golden=$PROBE_ROOT/crates/cfdcraft-backend-codesaturne/tests/golden/cavity-codesaturne

if [ -z "$image" ]; then
  echo "IMAGE is not set"
  echo "PROBE-DONE codesaturne-unconfigured"
  exit 2
fi

golden_ready "$golden" DATA/setup.xml
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
mkdir -p "$PROBE_OUT/SRC" "$PROBE_OUT/RESU"

run_container "$image" "$name-initialize" \
  'cd /w && code_saturne run --initialize --id probe-init > log.initialize 2>&1; echo "initialize exit status $?"'
run_container "$image" "$name-run" \
  'cd /w && code_saturne run --id probe-full > log.run 2>&1; echo "run exit status $?"'
run_container "$image" "$name-quality" \
  'cd /w && code_saturne run --quality --id probe-quality > log.quality 2>&1; echo "quality exit status $?"'

for log in log.initialize log.run log.quality; do
  excerpt "$name $log" "$PROBE_OUT/$log" 5 30
done

for d in "$PROBE_OUT"/RESU/*/; do
  [ -d "$d" ] || continue
  for f in run_solver.log setup.log summary; do
    [ -f "$d$f" ] && excerpt "$name $(basename "$d")/$f" "$d$f" 5 60
  done
done

find "$PROBE_OUT/RESU" -name 'error*' -type f -printf '%P\n' 2> /dev/null | LC_ALL=C sort > "$PROBE_LOGS/$name.errors"
echo "ERROR-FILES $(grep -c . "$PROBE_LOGS/$name.errors")"
print_text "$name error files" "$PROBE_LOGS/$name.errors"

while IFS= read -r f; do
  [ -f "$f" ] || continue
  rel=${f#"$PROBE_OUT"/}
  echo "HISTORY-FILE $rel $(wc -c < "$f") bytes"
  excerpt "$name $rel (first lines)" "$f" 4 2
  case "$rel" in
    */residuals.csv) print_text "$name $rel whole file" "$f" ;;
  esac
done < <(find "$PROBE_OUT/RESU" \( -name residuals.csv -o -path '*/monitoring/*' \) -type f | LC_ALL=C sort | head -n 20)

list_files "$PROBE_OUT" | head -n 200 > "$PROBE_LOGS/$name.files"
print_text "$name files (name and size, first 200)" "$PROBE_LOGS/$name.files"

echo "PROBE-DONE $name"
