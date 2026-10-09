# Graph-page loading

## Change

Release builds serve frontend assets with gzip/Brotli negotiation
(`api::frontend_router()`), and cache `/_app/immutable/` assets for one year.
HTML and non-hashed files use `no-cache`. Missing `/_app/` assets return 404
instead of the SPA HTML, so a stale hashed URL never caches HTML as immutable.

API and SSE routes are not compressed. The debug Vite proxy is unchanged.
Compression happens per request at default Brotli quality; precompressing at
build time is an option if serving CPU ever matters.

## Results

The graph route chunk (`nodes/2.*.js`) is 2.18 MB minified. ELK is about 66%
of it and Cytoscape about 20%.

| Graph chunk | Bytes |
| --- | ---: |
| identity | 2,177,742 |
| gzip | 659,217 |
| Brotli | 645,832 |

Cold load in headless Chrome, 10 Mbps, 40 ms latency, cache off, median of 5:

| | Before | After |
| --- | ---: | ---: |
| All `/_app/` transfer | 2.73 MB | 0.79 MB |
| Graph chunk request | 2,243 ms | 702 ms |
| First contentful paint | 2,348 ms | 812 ms |

Warm navigation transfers zero bytes for immutable assets. FCP here is the
loading shell with no backend, not a usable populated graph.

## Not measured

Time to a usable populated graph and main-thread stalls during ELK layout.
Measure these on a real campaign before considering lazy loading, a layout
worker, or a smaller layout engine.

## Reproduce

```sh
pnpm --prefix frontend build
cargo test -p api --release --test frontend_assets --locked
cargo test -p app --release --test router --locked
# Asset-only server for DevTools probes on 127.0.0.1:4178 (RAN_ASSET_PROBE_PORT)
cargo test -p api --release --test frontend_assets serve_static_assets -- --ignored --nocapture
```

These tests only run in release builds (debug builds proxy Vite). CI runs them
in the `test-release-assets` job.
