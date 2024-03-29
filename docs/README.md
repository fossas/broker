# User Manual

Broker is the bridge between FOSSA and internal projects.

FOSSA users use Broker to scan local projects,
importing them into the FOSSA service (including FOSSA in the cloud)
without sharing access to the source code of the project.

_Have a question not answered in the docs?_
_Check [the FAQ](./reference/faq.md) or send us a support request via [support.fossa.com](https://support.fossa.com)!_

## System requirements

Most modern systems can run Broker with no issues.
For a more detailed look at system requirements, see the [system requirements reference](./reference/system-requirements.md).

## Installing Broker

- To install Broker on your local system, see [install Broker locally](./reference/install-local.md)
- To run Broker in Kubernetes, see [install Broker in Kubernetes](./reference/install-kubernetes.md)

## Config

The Broker config file tells Broker about the repositories it should scan, how it can access them, and at what cadence.
See the [config reference](./reference/config.md) for more details.

## Subcommands

### `init`

Initialize an empty configuration file and database.

For more information, see the [`init` subcommand documentation](./subcommands/init.md).

### `fix`

Diagnose possible issues in the local runtime environment that may be preventing
Broker from scanning projects and sending their metadata to FOSSA.

For more information, see the [`fix` subcommand documentation](./subcommands/fix.md).

### `run`

Boots Broker using the local config file, scanning the projects on
configured DevOps hosts and importing their metadata into FOSSA.

For more information, see the [`run` subcommand documentation](./subcommands/run.md).
