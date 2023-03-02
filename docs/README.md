# User Manual

_TODO: Fill this out as we add functionality_

## Subcommands

### `init`

Initialize an empty configuration file and database.

### `fix`

Diagnose possible issues in the local runtime environment that may be preventing
Broker from scanning projects and sending their metadata to FOSSA.

### `run`

Boots Broker using the local config file, scanning the projects on
configured DevOps hosts and importing their metadata into FOSSA.

## Config

The Broker config file tells Broker about the repositories it should scan, how it can access them, and at what cadence.
See the [config reference](./reference/config.md) for more details.
