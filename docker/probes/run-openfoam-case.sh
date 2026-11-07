#!/usr/bin/env bash
# Runs one OpenFOAM backend's own golden cavity inside its image, the way the executor runs it, and
# prints what happened as text.
#
#   CASE_KIND=esi   IMAGE=<ESI image>   the exported cavity of the ESI writer, through its Allrun
#   CASE_KIND=of14  IMAGE=<image>       the Foundation cavity, through blockMesh, checkMesh and foamRun
#
# The image entrypoint is left alone on purpose: whether `bash -lc` reaches the solver through it, and
# whether the solver is on the login-shell path, are two of the facts this probe records. The exit
# status of a container is printed as a diagnostic; the logs and the field files are the evidence.

set -u
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

kind=${CASE_KIND:-}
image=${IMAGE:-}
if [ -z "$image" ]; then
  echo "IMAGE is not set"
  echo "PROBE-DONE openfoam-case-unconfigured"
  exit 2
fi

# report_case <name>: what the run left in /w.
report_case() {
  local name=$1 log f d
  list_files "$PROBE_OUT" > "$PROBE_LOGS/$name.files"
  print_text "$name files" "$PROBE_LOGS/$name.files"
  for f in "$PROBE_OUT"/log.*; do
    [ -f "$f" ] || continue
    log=$(basename "$f")
    echo "FACT $log End=$(grep -c '^End' "$f") MeshOK=$(grep -c 'Mesh OK' "$f") Fatal=$(grep -c 'FOAM FATAL' "$f") FloatingPoint=$(grep -ci 'floating point exception' "$f")"
    excerpt "$name $log" "$f" 12 30
  done
  grep -n -h -A8 'FOAM FATAL' "$PROBE_OUT"/log.* 2> /dev/null | head -n 60 > "$PROBE_LOGS/$name.errors"
  print_text "$name dictionary and fatal error text" "$PROBE_LOGS/$name.errors"
  : > "$PROBE_LOGS/$name.fields"
  for d in "$PROBE_OUT"/[0-9]*; do
    [ -d "$d" ] || continue
    [ "$(basename "$d")" = 0 ] && continue
    for f in "$d"/*; do
      [ -f "$f" ] && printf '%s/%s %s\n' "$(basename "$d")" "$(basename "$f")" "$(wc -c < "$f")" >> "$PROBE_LOGS/$name.fields"
    done
  done
  print_text "$name field files after the first time (name and size)" "$PROBE_LOGS/$name.fields"
}

case "$kind" in
  esi)
    name=openfoam-case-esi
    golden=$PROBE_ROOT/crates/cfdcraft-backend-esi/tests/golden
    golden_ready "$golden/cavity-v2512" constant system; a=$?
    golden_ready "$golden/cavity-v2512-export" Allrun; b=$?
    if [ "$a" -eq 2 ] || [ "$b" -eq 2 ]; then
      echo "PROBE-SKIPPED $name golden not reviewed"
      exit 0
    elif [ "$a" -ne 0 ] || [ "$b" -ne 0 ]; then
      echo "PROBE-SKIPPED $name golden not present"
      exit 0
    fi
    probe_begin "$name"
    print_image_facts "$image"
    # The case folder and the exported script live in two golden folders; together they are the exported case.
    cp -R "$golden/cavity-v2512/." "$PROBE_OUT/"
    cp -R "$golden/cavity-v2512-export/." "$PROBE_OUT/"
    run_container "$image" "$name-path" \
      'command -v blockMesh checkMesh simpleFoam; echo "WM_PROJECT_VERSION=${WM_PROJECT_VERSION:-unset}"'
    run_container "$image" "$name-allrun" 'cd /w && sh ./Allrun'
    report_case "$name"
    echo "PROBE-DONE $name"
    ;;
  of14)
    name=openfoam-case-of14
    golden=$PROBE_ROOT/crates/cfdcraft-backend-of14/tests/golden/cavity-of14
    golden_ready "$golden" constant system; a=$?
    if [ "$a" -eq 2 ]; then
      echo "PROBE-SKIPPED $name golden not reviewed"
      exit 0
    elif [ "$a" -ne 0 ]; then
      echo "PROBE-SKIPPED $name golden not present"
      exit 0
    fi
    probe_begin "$name"
    print_image_facts "$image"
    cp -R "$golden/." "$PROBE_OUT/"
    run_container "$image" "$name-path" \
      'command -v blockMesh checkMesh foamRun; echo "WM_PROJECT_VERSION=${WM_PROJECT_VERSION:-unset}"'
    run_container "$image" "$name-run" \
      'cd /w && blockMesh > log.blockMesh 2>&1; checkMesh > log.checkMesh 2>&1; foamRun > log.foamRun 2>&1; echo finished'
    report_case "$name"
    echo "PROBE-DONE $name"
    ;;
  *)
    echo "CASE_KIND must be esi or of14"
    echo "PROBE-DONE openfoam-case-unconfigured"
    exit 2
    ;;
esac
