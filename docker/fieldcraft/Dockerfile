# FieldCraft 0.7.5 command line tool, from the official Linux release (x86-64 only).
#
# Where each pin came from (looked up on 2026-11-06):
# - Base image: the index digest of debian:trixie-slim from the Docker Hub registry API.
# - Release tarball: the sha256 listed for the asset by the GitHub release API, equal to the
#   shasum -a 256 of our own download and to its line in SHA256SUMS.txt.
# - Signature: SHA256SUMS.txt.minisig, verified here against the release public key
#   RWQhq1vv7xD0/uAOt8fqERP1hP+1UCM9U9QHTDYTYytWKiuedTkT7Lgr (key id FEF410EFEF5BAB21, read from the
#   release notes and the signature file). The same check was done once outside the build with an
#   independent Ed25519 verification of the file and its trusted comment.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS build

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl minisign \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /dl
RUN curl -fsSL -O https://github.com/tangency-labs/fieldcraft/releases/download/v0.7.5/fieldcraft-0.7.5-linux-x86_64.tar.gz \
 && curl -fsSL -O https://github.com/tangency-labs/fieldcraft/releases/download/v0.7.5/SHA256SUMS.txt \
 && curl -fsSL -O https://github.com/tangency-labs/fieldcraft/releases/download/v0.7.5/SHA256SUMS.txt.minisig \
 && minisign -Vm SHA256SUMS.txt -x SHA256SUMS.txt.minisig -P RWQhq1vv7xD0/uAOt8fqERP1hP+1UCM9U9QHTDYTYytWKiuedTkT7Lgr \
 && grep ' fieldcraft-0.7.5-linux-x86_64.tar.gz$' SHA256SUMS.txt | sha256sum -c - \
 && echo "f07b957e79c22ed2bebaedf883f50085ef2aa81d477d7cf6bb839da94c72653e  fieldcraft-0.7.5-linux-x86_64.tar.gz" | sha256sum -c - \
 && mkdir -p /stage \
 && tar -xzf fieldcraft-0.7.5-linux-x86_64.tar.gz --strip-components=1 -C /stage \
      fieldcraft-0.7.5-linux-x86_64/bin/fieldcraft-cli \
      fieldcraft-0.7.5-linux-x86_64/share/doc/fieldcraft/LICENSE-MIT \
      fieldcraft-0.7.5-linux-x86_64/share/doc/fieldcraft/LICENSE-APACHE \
      fieldcraft-0.7.5-linux-x86_64/share/doc/fieldcraft/NOTICE \
      fieldcraft-0.7.5-linux-x86_64/share/doc/fieldcraft/THIRD-PARTY-LICENSES.txt \
 && chmod 0755 /stage/bin/fieldcraft-cli

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

LABEL org.opencontainers.image.title="FieldCraft 0.7.5 command line tool" \
      org.opencontainers.image.source="https://github.com/milleleaf/CFDCraft" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0" \
      com.milleleaf.solver.source="https://github.com/tangency-labs/fieldcraft/tree/v0.7.5"

COPY --from=build /stage/bin/fieldcraft-cli /opt/fieldcraft/bin/fieldcraft-cli
COPY --from=build /stage/share/doc/fieldcraft/ /opt/fieldcraft/licenses/

# A login shell resets the path, so the tool directory is added for it.
RUN echo 'export PATH=/opt/fieldcraft/bin:$PATH' > /etc/profile.d/fieldcraft.sh

# The tool never sends anything by itself; telemetry is switched off as well.
ENV FIELDCRAFT_TELEMETRY=0

# The run command is a login shell, so the image must not wrap it in an entrypoint.
ENTRYPOINT []
