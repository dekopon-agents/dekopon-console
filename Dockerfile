# Assembled from a published release: `container-image.yml` stages the verified Linux archives as
# `dist/<arch>/dekopon-console`, so nothing is compiled here and the image carries the exact bytes
# users download. Not buildable from the repository root.
#
# PID1 idles; operators start the console with `kubectl exec -it … -- dekopon-console`, which reads
# its broker socket, broker UID, catalog and subject from the container environment. UID 65535
# matches the core chart's console container, which reaches the broker socket through
# supplementary group 65534.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:54df941ed0d06a1bd95ef5e0ce391fd8d9f94b64782dc9a60062727849ee3f97

ARG TARGETARCH

COPY --chmod=0755 dist/${TARGETARCH}/dekopon-console /usr/local/bin/
COPY LICENSE-APACHE LICENSE-MIT /usr/share/doc/dekopon-console/

USER 65535:65535
WORKDIR /tmp

CMD ["dekopon-console", "--idle"]

LABEL org.opencontainers.image.title="dekopon-console" \
      org.opencontainers.image.description="Interactive terminal console for a running Dekopon broker" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0" \
      org.opencontainers.image.source="https://github.com/dekopon-agents/dekopon-console"
