# Namespace filtering

Ran can restrict which Kubernetes namespaces are considered during discovery.

Pass a config explicitly or place `ran.yaml` in the working directory:

```sh
ran emulate --config /path/to/ran.yaml
```

## Configuration

```yaml
namespaces:
  excluded:
    - kube-system
    - kube-public

  # When non-empty, included takes precedence over excluded.
  # included:
  #   - default
  #   - security-test
```

An `included` list is an allowlist. Otherwise, `excluded` is a denylist. Restart Ran after changing the file.

To verify the filter, start `ran emulate` and inspect the resources shown in the browser UI or returned by the REST API. Configuration errors are reported at startup; they are not silently replaced with historical defaults.

The `--namespace` CLI option is currently informational. Use `ran.yaml` for filtering.
