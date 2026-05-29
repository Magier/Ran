# Installation

## Binary (recommended)

Pre-built binaries for Linux, macOS (Intel and Apple Silicon), and Windows are
available on the [Releases page](https://github.com/magier/ran/releases/latest).

```sh
# macOS — Apple Silicon
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-arm64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# macOS — Intel
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# Linux (amd64)
curl -sL https://github.com/magier/ran/releases/latest/download/ran-linux-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/
```

Verify:

```sh
ran --version
```

## Docker

```sh
docker pull ghcr.io/magier/ran:latest
```

Run against your local kubeconfig:

```sh
docker run --rm -it \
  -v ~/.kube:/root/.kube:ro \
  -p 8080:8080 \
  ghcr.io/magier/ran:latest emulate --port 8080
```

Then open `http://localhost:8080`.

## Build from source

**Prerequisites:** Go 1.24+, Node.js 20+, pnpm

```sh
git clone https://github.com/magier/ran.git
cd ran
make build
./dist/ran --version
```
