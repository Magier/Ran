# Reading the Campaign Trail

Every execution contributes a step to the campaign flow. A step records the TTP summary, target, grounded command, status, success result, and output. Causal edges connect steps.

The browser flow view presents the current trail. **Save Ran JSON** downloads the same structure returned by:

```http
GET /api/flow
```

The current payload has `steps` and `edges` arrays. It is Ran's native JSON contract, not a STIX bundle.

Use the trail to compare each action with telemetry in your logging or SIEM platform. MITRE Attack Flow/STIX import and export remain roadmap work and will be implemented natively in Rust.
