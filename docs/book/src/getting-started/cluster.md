# Connecting to a Cluster

Ran uses your local kubeconfig to discover and target cluster resources. No
in-cluster agent or sidecar is required for most techniques.

> **Important:** Only run Ran against clusters you own or have explicit written
> authorisation to test.

## Default: use your current context

Ran reads `~/.kube/config` by default and uses whichever context `kubectl` would
use for the same operation. To check:

```sh
kubectl config current-context
```

## Namespace filtering

By default Ran shows every namespace. To reduce noise, create a `ran.yaml` in
your working directory:

```yaml
namespaces:
  # Hide system namespaces
  excluded:
    - kube-system
    - kube-public
    - kube-node-lease
```

Or use an allowlist instead (takes precedence over `excluded`):

```yaml
namespaces:
  included:
    - default
    - staging
```

Copy the example to get started:

```sh
cp ran.yaml.example ran.yaml
```

## Godmode

Pass `--godmode` to `ran emulate` if you want Ran to preload all cluster resources
from your kubeconfig on startup, rather than discovering them incrementally as you
run TTPs:

```sh
ran emulate --godmode
```

This is useful when you already have broad cluster access and want the full picture
immediately.

## What's next

With a cluster reachable, head to [The Armory](../armory/overview.md) to see what
techniques are available before running your first test.
