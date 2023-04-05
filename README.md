# Broker

The bridge between FOSSA and internal projects.

FOSSA users use Broker to scan local projects,
importing them into the FOSSA service (including FOSSA in the cloud)
without sharing access to the source code of the project!

## Quickstart

1. Install Broker ([documentation](https://github.com/fossas/broker/blob/main/docs/README.md))
2. Initialize Broker with `broker init`, which prints the location for `config.yml`
3. Configure the `config.yml` with your project(s)
4. Run Broker with `broker run`
5. Wait a little bit for import magic to happen and then view your projects in FOSSA!

For more information, see the [User Manual](https://github.com/fossas/broker/blob/main/docs/README.md).

## Supported Projects

Broker supports arbitrary project URLs:

| Kind  | Supported | Details                               |
|-------|-----------|---------------------------------------|
| `git` | ✅        | Any project reachable via `git clone` |

_Legend:_
- _✅: Supported_
- _⌛️: In Development_
- _🛑: Not Planned_

## System Requirements

Most modern systems can run Broker with no issues.
For a more detailed look at system requirements,
see the [system requirements here](https://github.com/fossas/broker/blob/main/docs/reference/system-requirements.md).

## Contributing

If you're interested in contributing, check out our [developer guide](https://github.com/fossas/broker/blob/main/docs/dev/README.md).
PRs are welcome and appreciated!
