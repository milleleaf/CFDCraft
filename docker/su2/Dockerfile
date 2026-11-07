# SU2 8.6.0, the official static OpenMP build for Linux (x86-64 only).
#
# Where each pin came from (looked up on 2026-11-06):
# - Base image: the index digest of debian:trixie-slim from the Docker Hub registry API.
# - Release zip: SU2_v8.6.0_linux64-omp.zip. The release carries no signature, so the checksum is the
#   sha256 listed for the asset by the GitHub release API (gh release view v8.6.0 --json assets),
#   which equals the shasum -a 256 of our own download.
# - Licence text: COPYING at tag v8.6.0, hashed with shasum -a 256 after our own download.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS build

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl unzip \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /dl
RUN curl -fsSL -o su2.zip https://github.com/su2code/SU2/releases/download/v8.6.0/SU2_v8.6.0_linux64-omp.zip \
 && echo "b306dcb7e76caec54e376542916aae14a32abd6c2f6fa8b871ec10ffe80b41ac  su2.zip" | sha256sum -c - \
 && curl -fsSL -o COPYING https://raw.githubusercontent.com/su2code/SU2/v8.6.0/COPYING \
 && echo "b3aa400aca6d2ba1f0bd03bd98d03d1fe7489a3bbb26969d72016360af8a5c9d  COPYING" | sha256sum -c - \
 && mkdir -p /stage/bin \
 && unzip -j su2.zip bin/SU2_CFD -d /stage/bin \
 && chmod 0755 /stage/bin/SU2_CFD

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

LABEL org.opencontainers.image.title="SU2 8.6.0 (OpenMP)" \
      org.opencontainers.image.source="https://github.com/milleleaf/CFDCraft" \
      org.opencontainers.image.licenses="LGPL-2.1-only" \
      com.milleleaf.solver.source="https://github.com/su2code/SU2/tree/v8.6.0"

COPY --from=build /stage/bin/SU2_CFD /opt/su2/bin/SU2_CFD
COPY --from=build /dl/COPYING /opt/su2/COPYING

# A login shell resets the path, so the solver directory is added for it.
RUN echo 'export PATH=/opt/su2/bin:$PATH' > /etc/profile.d/su2.sh

# The run command is a login shell, so the image must not wrap it in an entrypoint.
ENTRYPOINT []
