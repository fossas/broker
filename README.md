# Broker

The bridge between FOSSA and internal projects.

FOSSA users use Broker to scan local projects,
importing them into the FOSSA service (including FOSSA in the cloud)
without sharing access to the source code of the project!

## Quickstart

1. Install Broker: `TODO: add command`
2. Initialize Broker: `broker init`
3. Configure the `.broker.yml` with your projects
4. Run Broker: `broker run`
5. Wait a little bit for import magic to happen and then view your projects in FOSSA!

For more information, see the [User Manual](./docs/README.md).

## Supported Projects

Broker supports arbitrary project URLs:

| Kind  | Supported | Details                               |
|-------|-----------|---------------------------------------|
| `git` | ⌛️        | Any project reachable via `git clone` |

_Legend:_
- _✅: Supported_
- _⌛️: In Development_
- _🛑: Not Planned_

## Contributing

If you're interested in contributing, check out our [developer guide](./docs/dev/README.md).
PRs are welcome and appreciated!
