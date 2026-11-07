#!/usr/bin/env bash
# Helpers shared by the host-side probe scripts. Source this file; it runs nothing by itself.
#
# Every container of every probe starts through run_container, so one set of caps holds for all of
# them: no network, 3 CPUs, 12 GB, 512 processes, the probe scripts mounted read-only and one
# writable folder. Every captured file is printed through print_text, which cuts it at 200 KiB and
# removes the runner's home path and host name. The probes are meant for a hosted runner and are
# never run on a developer machine.

PROBE_ROOT=${PROBE_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}
PROBES_DIR=$PROBE_ROOT/docker/probes
PROBE_TOP=${PROBE_WORK:-${RUNNER_TEMP:-}}
if [ -z "$PROBE_TOP" ]; then
  echo "set PROBE_WORK or run on a hosted runner (RUNNER_TEMP)" >&2
  return 1 2>/dev/null || exit 1
fi
PROBE_LOGS=$PROBE_TOP/logs
PROBE_OUT=
PROBE_STATUS=0
PROBE_CAP_BYTES=204800

# probe_begin <name>: a fresh writable folder for one probe; the container sees it as /w.
probe_begin() {
  rm -rf "${PROBE_TOP:?}/$1"
  PROBE_OUT=$PROBE_TOP/$1/w
  mkdir -p "$PROBE_OUT" "$PROBE_LOGS"
  echo "PROBE-START $1"
}

# probe_redact: stdin to stdout without the runner's home path, workspace path and host name.
probe_redact() {
  local args=() v e
  for v in "${GITHUB_WORKSPACE:-}" "${HOME:-}" "$(hostname 2>/dev/null || true)"; do
    [ -n "$v" ] || continue
    e=$(printf '%s' "$v" | sed 's/[][\\.*^$|]/\\&/g')
    args+=(-e "s|$e|<redacted>|g")
  done
  if [ "${#args[@]}" -eq 0 ]; then cat; else sed "${args[@]}"; fi
}

# print_text <label> <file>: print one captured file as text, capped and redacted. The runner reads
# lines that start with two colons as commands, so printing stops command processing around the
# untrusted text.
print_text() {
  local label=$1 file=$2 size tok
  tok=$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')
  echo "::stop-commands::$tok"
  echo "----- $label"
  if [ -f "$file" ]; then
    size=$(wc -c < "$file")
    head -c "$PROBE_CAP_BYTES" "$file" | tr -d '\000-\010\013\014\015\016-\037\177' | LC_ALL=C probe_redact
    if [ "$size" -gt "$PROBE_CAP_BYTES" ]; then
      echo
      echo "(cut at 200 KiB; the file holds $size bytes)"
    fi
  else
    echo "(no such file)"
  fi
  echo "----- end $label"
  echo "::$tok::"
}

# excerpt <label> <file> <head lines> <tail lines>: the start and the end of a file.
excerpt() {
  local label=$1 file=$2 h=$3 t=$4 tmp
  tmp=$PROBE_LOGS/excerpt.$$.txt
  if [ -f "$file" ]; then
    { head -n "$h" "$file"; echo "[...]"; tail -n "$t" "$file"; } > "$tmp"
  else
    : > "$tmp"
  fi
  print_text "$label" "$tmp"
  rm -f "$tmp"
}

# list_files <folder>: relative file names with sizes in bytes, sorted.
list_files() {
  (cd "$1" 2>/dev/null && find . -type f -printf '%P %s\n' | LC_ALL=C sort)
}

# run_container <image> <name> <script>: run the script through a login shell in /w, the way the
# executor does, keep its output under <name>, and print status, standard output and standard error.
# The exit status is printed as a diagnostic only; it is never evidence of a good run.
# PROBE_ENTRYPOINT_RESET=1 clears the image entrypoint for a probe that does not test it.
run_container() {
  local image=$1 name=$2 script=$3 start extra=()
  [ -n "${PROBE_ENTRYPOINT_RESET:-}" ] && extra=(--entrypoint "")
  chmod -R a+rwX "$PROBE_OUT"
  start=$SECONDS
  docker run --rm --network none --cpus 3 --memory 12g --pids-limit 512 \
    -v "$PROBES_DIR:/probes:ro" -v "$PROBE_OUT:/w" \
    ${extra[@]+"${extra[@]}"} "$image" bash -lc "$script" \
    > "$PROBE_LOGS/$name.out" 2> "$PROBE_LOGS/$name.err"
  PROBE_STATUS=$?
  echo "CONTAINER $name exit status $PROBE_STATUS (diagnostic only), $((SECONDS - start)) s"
  print_text "$name standard output" "$PROBE_LOGS/$name.out"
  print_text "$name standard error" "$PROBE_LOGS/$name.err"
}

# print_image_facts <image>: entrypoint, command, user and architecture as the image declares them.
print_image_facts() {
  local f=$PROBE_LOGS/image-facts.$$.txt
  docker image inspect --format \
    'entrypoint {{json .Config.Entrypoint}}{{"\n"}}command {{json .Config.Cmd}}{{"\n"}}user {{json .Config.User}}{{"\n"}}architecture {{.Architecture}}' \
    "$1" > "$f" 2>&1
  print_text "image facts" "$f"
  rm -f "$f"
}

# golden_ready <folder> [<file>...]: 0 when the folder and the named files exist and no file holds
# the first-run placeholder; 2 when a placeholder is still there; 1 when the folder or a file is missing.
golden_ready() {
  local dir=$1 f
  shift
  [ -e "$dir" ] || return 1
  for f in "$@"; do
    [ -e "$dir/$f" ] || return 1
  done
  if grep -rIlF 'PENDING-FIRST-CI-RUN' "$dir" > /dev/null 2>&1; then
    return 2
  fi
  return 0
}
