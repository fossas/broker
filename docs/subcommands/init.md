# The `init` subcommand

_See [the FAQ](../reference/faq.md) for common questions related to this and other Broker functionality._

`broker init` creates the Broker data root on the local system and writes an initial configuration file at that location.

When `broker init` is run, Broker checks for whether a config file already exists at the data root.
- If so, broker creates a new config example at `$DATA_ROOT/config.example.yml`.
- If not, broker creates a new config file at `$DATA_ROOT/config.yml`, and example at `$DATA_ROOT/config.example.yml`.

_For more information on the config, see the [config reference](../reference/config.md)._

After `broker init` finishes, it reports the data root and these actions to the user.

## Subcommand FAQs

- [Where is the `DATA_ROOT`?](../reference/faq.md#where-is-the-data-root-for-broker)
- [Where is the config file stored?](../reference/faq.md#where-is-the-config-file-stored)
