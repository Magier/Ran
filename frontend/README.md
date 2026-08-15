# Ran frontend

The browser UI is a SvelteKit application embedded in the Rust server for release builds.

## Development

Install dependencies and start the frontend development server:

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Run `ran emulate` separately when working against the real API. REST is used for commands and server-sent events at `/events` provide live updates.

## Verification

```sh
pnpm test -- --run
pnpm build
```

The root `make build` command builds the frontend before compiling the Rust release binary.
