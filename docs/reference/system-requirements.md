# Reference: System requirements

These are the recommended system requirements.

Broker may run on less resources, but using Broker with lower system resources than listed here is unsupported.
Broker scales very well with more resources, so if analysis takes longer than you're hoping try bumping its CPU and memory limits.

Broker uses the FOSSA CLI, so its requirements are mainly dominated by how much CPU and memory FOSSA CLI needs.
If you already use FOSSA CLI on CI nodes, you can likely copy those resource limits for Broker.

## CPU

Broker requires a multi-core CPU.

- For containers, at least a 2000 mcpu limit.
- For running directly, at least dual core CPU.

## Memory

Broker requires at least 4 GB of memory.

- This can vary depending on the size of the project being scanned and the kind of project.

## Disk
  
Enough disk space to store a blobless clone of each configured code repository at the same time.
- Broker removes repositories after scanning, but it is possible they are all cloned at once.
- Further reading on [blobless clones here](https://github.blog/2020-12-21-get-up-to-speed-with-partial-clone-and-shallow-clone/).
- Further reading on [configured code repositories here](./config.md#integrations).

## Network

Must be able to reach your FOSSA instance (for most users this is `https://app.fossa.com`).
Must be able to access [configured code repositories](./config.md#integrations).
- These connections are strictly outbound.
- Do ensure that the firewall allows replies, often referred to as allowing "ESTABLISHED" and "RELATED" incoming connections.
  - Further reading on [allowing established and related connections here](https://www.digitalocean.com/community/tutorials/iptables-essentials-common-firewall-rules-and-commands#allowing-established-and-related-incoming-connections).
