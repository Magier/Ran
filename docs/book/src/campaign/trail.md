# Reading the Attack Trail

Every technique executed during a campaign is recorded in the **execution record**.
Together with the knowledge graph, it forms the complete audit trail of the
emulation session.

## What the execution record contains

For each invocation:

- The TTP ID, name, and tactic
- The target entity and its ID at execution time
- The procedure used and the grounded command
- The raw output (stdout + stderr)
- The entities and relations the execution produced
- A timestamp

## Viewing the trail in the UI

The **Timeline** panel (bottom of the `ran emulate` UI) shows the execution
record in chronological order. Click any entry to expand it and see the raw
output and the effects it produced.

The cluster map reflects the *cumulative* state — all entities and relations
discovered across the session. Use the timeline to step through how the graph
grew over time.

## Exporting as MITRE Attack Flow

Attack Flow is a MITRE CTID standard for representing sequences of adversary
actions as a STIX 2 graph. Ran can export the full campaign as an Attack Flow
document from within the web UI: **Export → Attack Flow (STIX 2)**.

The resulting file can be imported into the
[Attack Flow Builder](https://center-for-threat-informed-defense.github.io/attack-flow/ui/)
for visualisation, shared with blue team stakeholders, or used as input to replay
the same sequence in a future session.

## Using the trail for detection validation

The primary purpose of the attack trail is to validate your detection stack. After
a campaign session:

1. Open your SIEM or log platform.
2. For each TTP in the execution record, search for the expected detection signal.
3. Annotate the Attack Flow export: mark which steps triggered detections and which
   went undetected.
4. Repeat the campaign with variations on technique parameters or procedures to
   test edge cases in your detection rules.
