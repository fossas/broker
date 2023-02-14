# Broker

The bridge between FOSSA and internal DevOps services.

Using Broker, FOSSA users may scan local projects in internal DevOps hosts,
importing them into the FOSSA service (including FOSSA in the cloud)
without sharing access to the source code of the project.

## Quickstart

1. Install Broker: `TODO: add command`
2. Initialize Broker: `broker init`
3. Configure the `.broker.yml` with your DevOps host or project URLs
4. Run Broker: `broker run`
5. View your projects in FOSSA!

For more information, see the [User Manual](./docs/README.md).

## Supported DevOps Hosts

DevOps hosts are services which host many repositories.
Broker supports the following DevOps hosts:

| Host       | Supported | Details                     |
|------------|-----------|-----------------------------|
| github.com | ⌛️        | The GitHub SaaS application |
| gitlab.com | ⌛️        | The GitLab SaaS application |

Additionally, Broker supports arbitrary project URLs:

| Kind  | Supported | Details                               |
|-------|-----------|---------------------------------------|
| `git` | ⌛️        | Any project reachable via `git clone` |
