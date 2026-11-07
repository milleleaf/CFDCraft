#!/usr/bin/env bash
# Runs inside an OpenFOAM image. Lists the case dictionaries the image ships and names which of them
# hold a construct that runs code or loads a library. It prints the construct only, never the line.
#
#   LIST <path>              one regular file under etc/caseDicts, relative to the project folder
#   HIT <path> <construct>   the file contains that construct
#
# Nothing is written and nothing is run besides grep and find.

echo "ARCH $(uname -m)"

# A login shell reaches the project environment in some images and not in others; look for it.
if [ -z "${WM_PROJECT_DIR:-}" ]; then
  for rc in /usr/lib/openfoam/openfoam*/etc/bashrc /openfoam/etc/bashrc /opt/openfoam*/etc/bashrc; do
    if [ -f "$rc" ]; then
      set +u
      # shellcheck disable=SC1090
      . "$rc" > /dev/null 2>&1
      echo "ENVIRONMENT sourced from the image's own bashrc"
      if [ -z "${WM_PROJECT_DIR:-}" ]; then
        WM_PROJECT_DIR=$(dirname "$(dirname "$rc")")
      fi
      break
    fi
  done
else
  echo "ENVIRONMENT present in the login shell"
fi

echo "PROJECT_VERSION ${WM_PROJECT_VERSION:-unset}"
echo "PROJECT_DIR ${WM_PROJECT_DIR:-unset}"
dpkg -l 2> /dev/null | awk '$1 == "ii" && $2 ~ /^openfoam/ { print "PACKAGE " $2 " " $3 }'

if [ -z "${WM_PROJECT_DIR:-}" ] || [ ! -d "$WM_PROJECT_DIR/etc/caseDicts" ]; then
  echo "NO-CASEDICTS the project folder or its etc/caseDicts folder was not found"
  echo "PROBE-DONE etc-listing"
  exit 0
fi

cd "$WM_PROJECT_DIR" || exit 0

files=$(find etc/caseDicts -type f | LC_ALL=C sort)
echo "$files" | while IFS= read -r p; do
  [ -n "$p" ] && echo "LIST $p"
done

hits=0
while IFS= read -r p; do
  [ -n "$p" ] || continue
  grep -qsF '#calc' "$p" && { echo "HIT $p #calc"; hits=$((hits + 1)); }
  grep -qsF 'codeStream' "$p" && { echo "HIT $p codeStream"; hits=$((hits + 1)); }
  grep -qsF 'systemCall' "$p" && { echo "HIT $p systemCall"; hits=$((hits + 1)); }
  grep -qsE 'type[[:space:]]+coded' "$p" && { echo "HIT $p type coded"; hits=$((hits + 1)); }
  grep -qsF 'libs' "$p" && { echo "HIT $p libs"; hits=$((hits + 1)); }
done <<< "$files"

echo "COUNT files $(echo "$files" | grep -c .) hits $hits"
echo "PROBE-DONE etc-listing"
