# === Dev Setup ===
.PHONY: install-hooks
install-hooks:
	@printf '#!/bin/sh\nset -e\ncargo fmt --check || { echo "Run: make fmt"; exit 1; }\ncargo clippy --workspace --locked -- -D warnings\n' > .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "pre-commit hook installed"

.PHONY: dev-workspace
dev-workspace:
	./scripts/dev-workspace.zsh

# === Code Generation ===
.PHONY: generate-api
generate-api:
	cd frontend && pnpm exec openapi-typescript ../api/openapi.yaml -o src/lib/api/gen_types.ts

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
	cargo clippy --workspace --locked -- -D warnings

.PHONY: lint
lint: fmt-check clippy

# === Testing ===
.PHONY: test-rust
test-rust:
	cargo test --workspace --locked

.PHONY: test-frontend
test-frontend:
	pnpm --prefix frontend test

.PHONY: test
test: test-rust test-frontend

# === Builds ===
.PHONY: build-frontend
build-frontend:
	pnpm --prefix frontend build

# armory/TTPs is embedded into release binaries through bundled-armory.
RUST_RELEASE_FLAGS := --package cli --features cli/bundled-armory

.PHONY: build build-rust
build build-rust: build-frontend
	cargo build --locked --release $(RUST_RELEASE_FLAGS)

.PHONY: build-rust-target
build-rust-target: build-frontend
ifndef RUST_TARGET
	$(error RUST_TARGET is not set, e.g. RUST_TARGET=x86_64-unknown-linux-gnu)
endif
	cargo build --locked --release $(RUST_RELEASE_FLAGS) --target $(RUST_TARGET)

# === Containers ===
.PHONY: docker-local
docker-local:
	docker build -f Dockerfile.dev -t ran:local .

.PHONY: docker-run
docker-run:
	docker run --rm -it -p 8080:8080 -v "$$HOME/.kube:/root/.kube:ro" ran:local
