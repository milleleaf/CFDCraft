# code_saturne 9.2.0 as published by its maintainers (amd64 only), with the entrypoint and the
# login-shell path reset.
#
# Where the pin came from (looked up on 2026-11-06): the manifest digest of simvia/code_saturne:9.2.0
# from the Docker Hub registry API (a manifest request for the tag, reading Docker-Content-Digest).
# The base image ends as the user code_saturne (read from its configuration blob), restored below.
# Nothing is downloaded in this file.
FROM simvia/code_saturne@sha256:7639bc95baf6e5c284f4e67ea418ae6d46809410b7e04ed477983a8e39513ed5

LABEL org.opencontainers.image.title="code_saturne 9.2.0 (wrapper)" \
      org.opencontainers.image.source="https://github.com/milleleaf/CFDCraft" \
      org.opencontainers.image.licenses="GPL-2.0-or-later" \
      com.milleleaf.solver.source="https://github.com/code-saturne/code_saturne/archive/refs/tags/v9.2.0.tar.gz"

# The base image ends as an unprivileged user; the path script is written as root.
USER root

# A login shell resets the path, so the solver and MPI directories are added for it.
RUN echo 'export PATH=/opt/code_saturne/bin:/opt/openmpi/bin:$PATH' > /etc/profile.d/codesaturne.sh

# The base image starts the solver program itself, which would swallow the run command.
ENTRYPOINT []

USER code_saturne
