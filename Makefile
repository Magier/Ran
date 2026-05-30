# === Dev Setup ===
.PHONY: install-hooks
install-hooks:
	@printf '#!/bin/sh\nset -e\ncargo fmt --check || { echo "Run: make fmt"; exit 1; }\ncargo clippy --workspace -- -D warnings\n' > .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "pre-commit hook installed"

# === Code Generation ===
.PHONY: generate-api
generate-api:
	cd legacy/src && go run github.com/oapi-codegen/oapi-codegen/v2/cmd/oapi-codegen@latest -package api -generate chi-server,models,embedded-spec -o api/api.gen.go api/openapi.yaml
	cd frontend && pnpm exec openapi-typescript ../legacy/src/api/openapi.yaml -o src/lib/api/gen_types.ts

.PHONY: generate
generate: generate-api

# === Linting ===
.PHONY: fmt
fmt:
	cargo fmt

.PHONY: fmt-check
fmt-check:
	cargo fmt --check

.PHONY: clippy
clippy:
	cargo clippy --workspace -- -D warnings

.PHONY: lint
lint: fmt-check clippy

# === Testing ===
.PHONY: test-go
test-go:
	cd legacy/src && go test -v ./...

.PHONY: test-rust
test-rust:
	cargo test --workspace

.PHONY: test-frontend
test-frontend:
	pnpm --prefix frontend test

# === Asset Preparation ===
.PHONY: build-frontend
build-frontend:
	cd frontend && pnpm run build

.PHONY: copy-armory
copy-armory:
	mkdir -p legacy/src/armory/builtin
	rm -rf legacy/src/armory/builtin/*
	cp -a armory/TTPs/. legacy/src/armory/builtin/

.PHONY: copy-frontend
copy-frontend:
	mkdir -p legacy/src/api/static
	cp -r frontend/build/. legacy/src/api/static/

.PHONY: prepare-assets
prepare-assets: copy-armory copy-frontend

# === Rust Release Builds ===
# armory/TTPs is embedded into the binary via the bundled-armory feature.
# Do NOT pass --features bundled-armory for dev/debug builds.
RUST_RELEASE_FLAGS := --package cli --features cli/bundled-armory

.PHONY: build-rust
build-rust:
	cargo build --release $(RUST_RELEASE_FLAGS)

.PHONY: build-rust-target
build-rust-target:
ifndef RUST_TARGET
	$(error RUST_TARGET is not set, e.g. RUST_TARGET=x86_64-unknown-linux-gnu)
endif
	cargo build --release $(RUST_RELEASE_FLAGS) --target $(RUST_TARGET)

# === Legacy Go Builds ===


.PHONY: build-binary
build-binary: prepare-assets
ifndef GOOS
	$(error GOOS is not set)
endif
ifndef GOARCH
	$(error GOARCH is not set)
endif
	mkdir -p dist/$(GOOS)-$(GOARCH)
	cd legacy/src && \
	DEST=../../dist/$(GOOS)-$(GOARCH)/ran$(if $(filter windows,$(GOOS)),.exe,) && \
	CGO_ENABLED=0 GOOS=$(GOOS) GOARCH=$(GOARCH) go build -o $$DEST . && chmod +x $$DEST

# Local development


.PHONY: build
build: prepare-assets
	cd legacy/src && go build -o ../../dist/ran . && chmod +x ../../dist/ran

.PHONY: build-all
build-all: prepare-assets
	$(MAKE) build-binary GOOS=darwin GOARCH=amd64
	$(MAKE) build-binary GOOS=darwin GOARCH=arm64
	$(MAKE) build-binary GOOS=linux GOARCH=amd64
	$(MAKE) build-binary GOOS=linux GOARCH=arm64
	$(MAKE) build-binary GOOS=windows GOARCH=amd64

# CI target
.PHONY: ci-build
ci-build: prepare-assets build-binary

# === Local Docker ===
UNAME_M := $(shell uname -m)
ifeq ($(UNAME_M),arm64)
    LOCAL_ARCH := arm64
else
    LOCAL_ARCH := amd64
endif

.PHONY: docker-local
docker-local: prepare-assets
	$(MAKE) build-binary GOOS=linux GOARCH=$(LOCAL_ARCH)
	docker build --build-arg TARGETARCH=$(LOCAL_ARCH) -f Dockerfile.release -t ran:local .

.PHONY: docker-run
docker-run:
	docker run --rm -p 8080:8080 ran:local