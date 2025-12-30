# === Testing ===
.PHONY: test-go
test-go:
	cd src && go test -v ./...

.PHONY: test-frontend
test-frontend:
	pnpm --prefix frontend test

# === Asset Preparation ===
.PHONY: copy-armory
copy-armory:
	mkdir -p src/armory/builtin
	rm -rf src/armory/builtin/*
	cp -a armory/TTPs/. src/armory/builtin/

.PHONY: copy-frontend
copy-frontend:
	mkdir -p src/api/static
	cp -r frontend/build/. src/api/static/

.PHONY: prepare-assets
prepare-assets: copy-armory copy-frontend

# === Building ===
.PHONY: build-binary
build-binary:
ifndef GOOS
	$(error GOOS is not set)
endif
ifndef GOARCH
	$(error GOARCH is not set)
endif
	mkdir -p dist/$(GOOS)-$(GOARCH)
	cd src && CGO_ENABLED=0 GOOS=$(GOOS) GOARCH=$(GOARCH) DEST=../dist/$(GOOS)-$(GOARCH)/ran$(if $(filter windows,$(GOOS)),.exe,) go build -o $DEST . && chmod +x $DEST

# Local development
.PHONY: build
build: prepare-assets
	cd src && go build -o ../dist/ran . && chmod +x ../dist/ran

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