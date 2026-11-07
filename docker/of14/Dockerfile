# OpenFOAM Foundation version 14, package 20261008, on Ubuntu 26.04.
#
# Where each pin came from (looked up on 2026-11-06):
# - Base image: the index digest of ubuntu:26.04 from the Docker Hub registry API (a manifest
#   request for the tag, reading the Docker-Content-Digest header). The index covers amd64 and arm64.
# - Repository key: https://dl.openfoam.org/gpg.key, downloaded and hashed with shasum -a 256.
#   It carries the fingerprint 678184F120A87A475F6569726C0DAC728B29D817 (gpg --show-keys).
# - Package: the Packages index of the resolute suite lists openfoam14 20261008 for amd64 and arm64.
FROM ubuntu:26.04@sha256:6a06111c3b0a8600feadb270aec771dc239bf407b25625f93f44ab00e11f4d3c

LABEL org.opencontainers.image.title="OpenFOAM Foundation 14 (package 20261008)" \
      org.opencontainers.image.source="https://github.com/milleleaf/CFDCraft" \
      org.opencontainers.image.licenses="GPL-3.0" \
      com.milleleaf.solver.source="https://dl.openfoam.org/source/14.20261008"

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -fsSL -o /usr/share/keyrings/openfoam.asc https://dl.openfoam.org/gpg.key \
 && echo "fbaacfe37e704494f6e65ab424e67b031191769e84c31185acb701f9e74fe0a9  /usr/share/keyrings/openfoam.asc" | sha256sum -c - \
 && echo "deb [signed-by=/usr/share/keyrings/openfoam.asc] https://dl.openfoam.org/ubuntu resolute main" > /etc/apt/sources.list.d/openfoam.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends openfoam14=20261008 \
 && rm -rf /var/lib/apt/lists/*

# A login shell resets the path, so the solver environment is added for it.
RUN echo '[ -f /opt/openfoam14/etc/bashrc ] && . /opt/openfoam14/etc/bashrc' > /etc/profile.d/openfoam14.sh

# The run command is a login shell, so the image must not wrap it in an entrypoint.
ENTRYPOINT []
