#!/usr/bin/env bash
# Runs `surfaceFeatures` on a tiny surface in the Foundation image at two package versions and prints
# what each run did. The newer package is the one the image pins; the older one is the package on which
# the tool was seen to stop with a floating-point exception.
#
#   IMAGE_NEW=<image at the pinned package>   IMAGE_OLD=<image at the older package, may be empty>
#
# The surface and the dictionaries are written here from the format's own facts: a unit cube as an
# ASCII STL (twelve triangles, outward normals), a control dictionary with the few entries the tool
# reads, and a features dictionary that names the surface. The exit status is printed as a diagnostic
# only: a crash can leave a zero status and a good run is judged by its log and its files.

set -u
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

if [ -z "${IMAGE_NEW:-}" ]; then
  echo "IMAGE_NEW is not set"
  echo "PROBE-DONE surfacefeatures-unconfigured"
  exit 2
fi

# facet <nx> <ny> <nz> <a> <b> <c>: one STL triangle; corners are named by their 0/1 coordinates.
facet() {
  local n="$1 $2 $3" c
  echo "  facet normal $n"
  echo "    outer loop"
  for c in "$4" "$5" "$6"; do
    echo "      vertex ${c:0:1} ${c:1:1} ${c:2:1}"
  done
  echo "    endloop"
  echo "  endfacet"
}

write_case() {
  local d=$1
  mkdir -p "$d/system" "$d/constant/triSurface"
  {
    echo "solid cube"
    facet 0 0 -1 000 010 110
    facet 0 0 -1 000 110 100
    facet 0 0 1 001 101 111
    facet 0 0 1 001 111 011
    facet 0 -1 0 000 100 101
    facet 0 -1 0 000 101 001
    facet 0 1 0 010 011 111
    facet 0 1 0 010 111 110
    facet -1 0 0 000 001 011
    facet -1 0 0 000 011 010
    facet 1 0 0 100 110 111
    facet 1 0 0 100 111 101
    echo "endsolid cube"
  } > "$d/constant/triSurface/cube.stl"
  cat > "$d/system/controlDict" << 'EOF'
FoamFile
{
    format      ascii;
    class       dictionary;
    object      controlDict;
}

application     surfaceFeatures;
startFrom       startTime;
startTime       0;
stopAt          endTime;
endTime         1;
deltaT          1;
writeControl    timeStep;
writeInterval   1;
EOF
  cat > "$d/system/surfaceFeaturesDict" << 'EOF'
FoamFile
{
    format      ascii;
    class       dictionary;
    object      surfaceFeaturesDict;
}

surfaces        ("cube.stl");
includedAngle   150;
writeObj        yes;
EOF
}

# one_run <label> <image>
one_run() {
  local label=$1 image=$2 name=surfacefeatures-$1
  probe_begin "$name"
  echo "RUN $label"
  write_case "$PROBE_OUT"
  run_container "$image" "$name-package" \
    'dpkg-query -W -f "PACKAGE \${Package} \${Version}\n" openfoam14; uname -m'
  run_container "$image" "$name-run" \
    'cd /w && surfaceFeatures > log.surfaceFeatures 2>&1; echo "tool exit status $?"'
  echo "NOTE the exit status of the tool is not evidence; the last log lines and the files below are"
  tail -n 20 "$PROBE_OUT/log.surfaceFeatures" > "$PROBE_LOGS/$name.tail" 2> /dev/null
  print_text "$name last 20 log lines" "$PROBE_LOGS/$name.tail"
  list_files "$PROBE_OUT" > "$PROBE_LOGS/$name.files"
  print_text "$name files" "$PROBE_LOGS/$name.files"
}

if ! docker image inspect "$IMAGE_NEW" > /dev/null 2>&1; then
  echo "PROBE-SKIPPED surfacefeatures the image was not built"
  exit 0
fi

one_run new "$IMAGE_NEW"

if [ -n "${IMAGE_OLD:-}" ] && docker image inspect "$IMAGE_OLD" > /dev/null 2>&1; then
  one_run old "$IMAGE_OLD"
else
  echo "OLD-IMAGE-MISSING the older package could not be installed, so only the pinned package was run"
fi

echo "PROBE-DONE surfacefeatures"
