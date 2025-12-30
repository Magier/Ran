# Build stage - uses Makefile as source of truth
FROM golang:1.24-alpine AS builder

# Install build dependencies
RUN apk add --no-cache make nodejs npm && npm install -g pnpm

WORKDIR /app

# Copy everything needed for the build
COPY Makefile ./
COPY frontend/ ./frontend/
COPY src/ ./src/
COPY armory/ ./armory/

# Install frontend dependencies
RUN pnpm --prefix frontend install --frozen-lockfile

# Run the Makefile build target
RUN CGO_ENABLED=0 GOOS=linux make build

# Final minimal stage
FROM golang:trixie

ARG TARGETARCH

COPY dist/linux-${TARGETARCH}/ran /ran

EXPOSE 8080

ENTRYPOINT ["/ran"]
