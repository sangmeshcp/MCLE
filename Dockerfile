# Expects prebuilt static binaries from `cargo xtask dist` in dist/bin/<os>-<arch>/mcle.
FROM gcr.io/distroless/static-debian12:nonroot
ARG TARGETOS
ARG TARGETARCH
COPY dist/bin/${TARGETOS}-${TARGETARCH}/mcle /usr/local/bin/mcle
ENV MCLE_BIND=0.0.0.0
EXPOSE 4700
HEALTHCHECK --interval=5s --timeout=2s --retries=12 CMD ["/usr/local/bin/mcle", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/mcle"]
CMD ["start"]
