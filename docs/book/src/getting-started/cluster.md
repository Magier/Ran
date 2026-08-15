# Connecting to a Cluster

Ran uses the active kubeconfig context by default. Supply another file with `--kubeconfig`:

```sh
ran emulate --kubeconfig /path/to/config
```

Only run Ran against clusters you own or are authorised to test.

## Namespace filtering

Create `ran.yaml` or pass it with `--config`:

```yaml
namespaces:
  excluded:
    - kube-system
    - kube-public
```

An `included` list acts as an allowlist and takes precedence over `excluded`. See [namespace filtering](../../NAMESPACE_FILTERING.md).

Ran builds campaign knowledge through the selected initial-access action and subsequent discovery. There is no broad-access or “god mode” CLI switch.
