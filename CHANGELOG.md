
## v0.1.0

Broker MVP release

- Supports `git` integrations.
- Supports `broker init` to set up Broker's config files.
- Supports `broker run` to actually run Broker.

`broker run` consists of:
- Polling configured `git` integrations.
- For each integration, scanning:
  - New or changed branches since the last scan.
  - New tags since the last scan.
  - In both cases, only tags pushed in the last 30 days, or branches pushed to in the last 30 days, are considered.

`broker init` consists of:
- Creating the Broker data location.
- Creating example Broker config files.

See the [Broker User Manual](./docs/README.md) for more help using Broker.
