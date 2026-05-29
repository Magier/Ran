# When to Write a Custom TTP

The built-in armory covers ~80 techniques across the MITRE ATT&CK tactic spectrum
for Kubernetes. Before writing a new one, check whether an existing technique can
be adapted:

- **Use `--param` overrides** — many techniques are parameterised enough that
  changing a command string or target address covers your use case.
- **Check the `disabled` techniques** — `ran armory --all` lists disabled TTPs.
  A technique may already exist but be disabled because its PoC binary isn't
  bundled. You can enable it via a custom YAML override.
- **Check custom armory support** — if you have a slightly different variant of
  an existing technique, override it by placing a YAML with the same `id` in your
  custom armory directory.

Write a new TTP when:

- You need a technique for a CVE or exploit chain that doesn't exist in the armory.
- You want to test a custom application-level vulnerability specific to your environment.
- You need a technique from a tactic not yet covered (e.g. a new cloud provider API attack).
- You want to model internal tooling that wouldn't be appropriate in the public armory.

## File location and naming

Place custom TTPs under the relevant tactic directory in your custom armory:

```text
my-ttps/
└── Privilege Escalation/
    └── exploit_internal_webhook.yaml
```

The filename becomes the default TTP ID (kebab-cased). The tactic is inferred from
the directory name unless a `tactic:` field overrides it.

## Minimal valid TTP

The only required field is `name`. A TTP with just `name` and a `procedures` list
is immediately runnable:

```yaml
name: Check Node Hostname
procedures:
  - key: shell
    command: hostname
```

Add `tactic`, `techniques`, `preconditions`, `parameters`, and `effects` as you
refine it. See [TTP Anatomy](../armory/anatomy.md) for the full field reference.

## Testing your TTP

After writing the YAML:

```sh
# Confirm it parses and appears in the armory
ran armory --armory ./my-ttps

# Invoke it
ran invoke check-node-hostname --armory ./my-ttps --target default/test-pod
```

If the TTP doesn't appear in `ran armory`, check for YAML syntax errors and
ensure the `name:` field is present.
