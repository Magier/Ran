FROM ubuntu:24.04

ARG TARGETARCH

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY dist/linux-${TARGETARCH}/ran /usr/local/bin/ran
RUN chmod +x /usr/local/bin/ran

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/ran"]
CMD ["emulate", "--port", "8080"]
