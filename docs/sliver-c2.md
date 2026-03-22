# Sliver C2 Integration

Ran has basic experimental support for [Sliver](https://github.com/BishopFox/sliver) as an external C2 framework. When Sliver is configured, Ran acts as a Sliver *operator* client, delegating the execution of implant-based procedures to the Sliver server rather than running them directly.

> [!WARNING]
> Sliver integration is incomplete and actively in development. See [Milestones](../Milestones.md) for planned work.

---

## How it works

In the default configuration Ran uses its own lightweight built-in C2. When a Sliver configuration is provided, Ran connects to a running Sliver server and can direct implants to execute technique procedures through that channel.

```
┌─────────┐   operator config    ┌──────────────┐   implant comms   ┌─────────┐
│   Ran   │ ──────────────────►  │ Sliver server│ ────────────────► │ Target  │
└─────────┘                      └──────────────┘                   └─────────┘
```

---

## Prerequisites

- A running Sliver server (local or remote)
- An operator configuration file generated for Ran

---

## Setup

### 1. Generate an operator config on the Sliver server

Follow the [Sliver multi-player mode instructions](https://sliver.sh/docs?name=Multi-player+Mode) to create a new operator. The operator config bundles the server address and mTLS certificates needed for Ran to connect.

```
[server] sliver > multiplayer
[server] sliver > new-operator --name ran --lhost <server-ip> --save /tmp/ran_operator.cfg
```

Copy the generated config to the machine running Ran.

### 2. Place the config file

Ran currently expects the operator configuration to be named `sliver_cfg.json` and located in the **same directory as the `ran` binary** (or the working directory when running via Docker).

```sh
cp /tmp/ran_operator.cfg ./sliver_cfg.json
```

### 3. Start Ran

```sh
ran emulate
```

Ran will detect the `sliver_cfg.json` and attempt to connect to the Sliver server at startup.

---

## Current limitations

- Only a subset of Ran TTPs have Sliver-backed procedures implemented
- The operator config path is hardcoded to `sliver_cfg.json` — no flag or config-file option yet
- Session/implant selection is not yet surfaced in the UI; the first available session is used

---

## References

- [Sliver documentation](https://sliver.sh/docs)
- [Sliver multi-player mode](https://sliver.sh/docs?name=Multi-player+Mode)
- [BishopFox/sliver on GitHub](https://github.com/BishopFox/sliver)
