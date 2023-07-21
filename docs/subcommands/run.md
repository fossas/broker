# The `run` subcommand

`broker run` starts Broker and imports the projects specified in the config.

_See [the FAQ](../reference/faq.md) for common questions related to this and other Broker functionality._
_For more information on the config, see the [config reference](../reference/config.md)._

## Subcommand FAQs

- [Where is the `$DATA_ROOT`?](../reference/faq.md#where-is-the-data-root-for-broker)
- [Where is the config file stored?](../reference/faq.md#where-is-the-config-file-stored)
- [Where are debugging artifacts stored?](../reference/faq.md#where-are-debug-artifacts-stored)
- [Does Broker understand FOSSA CLI config files?](../reference/faq.md#does-broker-understand-fossa-cli-config-files-checked-into-the-repository-being-scanned)
- [What is scanned in a `git` integration?](../reference/faq.md#what-is-scanned-from-a-git-integration-during-broker-run)

## Scan upload rate limiting

`broker run` rate limits scans. The rate limiting is as follows:

- Broker uploads at most one scan per configured integration per minute.
- Scan results for each integration are enqueued while waiting for upload.
- If the queue becomes full, Broker pauses working on additional scans for that integration until the upload queue has space again.

Given this, if a given Broker instance has 5 integrations, it would upload at most 5 scan results per minute (one for each integration).
If each of those integrations have 3 revisions being uploaded, Broker will take at most 3 minutes to upload them all (one scan per minute, per revision).
