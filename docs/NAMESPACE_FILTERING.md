# Namespace Filtering Configuration

The namespace filtering feature allows you to control which Kubernetes namespaces are included when Ran discovers pods in your cluster.

## Configuration Location

The configuration file can be specified in two ways:

1. **Command-line flag**: Use `--config` flag when running Ran
   ```bash
   ran emulate --config /path/to/your/ran.yaml
   ```

2. **Default location**: Place `ran.yaml` in the current working directory

If no `--config` flag is provided, Ran will look for `ran.yaml` in the current working directory. If not found, it will use default configuration.

## Configuration Format

The configuration uses YAML format with the following structure:

```yaml
namespaces:
  # List of namespaces to exclude (blacklist)
  excluded:
    - namespace1
    - namespace2

  # Or, list of namespaces to include (whitelist)
  included:
    - namespace3
    - namespace4
```

**Note:** If `included` has items, it takes precedence over `excluded` (whitelist mode).

## Modes

### Exclude Mode (Blacklist)

Hides specific namespaces from discovery. All other namespaces will be shown.

**Example:**
```yaml
namespaces:
  excluded:
    - kube-system
    - local-path-storage
    - kube-public
    - kube-node-lease
```

This will show all namespaces **except** the ones in the excluded list.

### Include Only Mode (Whitelist)

Shows only the specified namespaces. All other namespaces will be hidden.

**Example:**
```yaml
namespaces:
  included:
    - default
    - production
    - staging
```

This will **only** show namespaces in the included list, hiding everything else.

## Default Behavior

If no configuration file exists, Ran uses the following defaults:

```yaml
namespaces:
  excluded:
    - kube-system
    - local-path-storage
```

## Testing Your Configuration

You can verify your configuration is working by:

1. Running `ran emulate` against your cluster
2. Checking which namespaces appear in the TUI or API responses
3. Comparing against the namespaces in your cluster (`kubectl get namespaces`)

## Example Use Cases

### Scenario 1: Security Testing in Production

Only test against specific environments:

```yaml
namespaces:
  included:
    - security-test-1
    - security-test-2
```

### Scenario 2: Excluding All System Namespaces

Hide all Kubernetes system namespaces:

```yaml
namespaces:
  excluded:
    - kube-system
    - kube-public
    - kube-node-lease
    - local-path-storage
    - metallb-system
    - ingress-nginx
    - cert-manager
```

### Scenario 3: Development Environment

Focus only on your development namespace:

```yaml
namespaces:
  included:
    - default
```

## Notes

- The configuration is loaded when `GetIDsOfRunningPods()` is called
- Changes to the config file require restarting Ran to take effect
- If `included` is non-empty, it acts as a whitelist (only those namespaces are shown)
- If `included` is empty, `excluded` acts as a blacklist (those namespaces are hidden)
- If the config file has syntax errors, Ran falls back to default configuration
