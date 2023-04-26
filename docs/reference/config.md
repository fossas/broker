# Reference: Config

The Broker config file tells Broker about the repositories it should scan, how it can access them, and at what cadence.

## Quick Start

For more detail, see the sections below.
The format is as follows:

```yaml
fossa_endpoint: https://app.fossa.com
fossa_integration_key: abcd1234
version: 1

debugging:
  location: /home/me/.config/fossa/broker/debugging/
  retention:
    days: 7

integrations:
- type: git
  poll_interval: 1h
  url: git@github.com:fossas/broker.git
  auth:
    type: ssh_key_file
    path: /home/me/.ssh/id_rsa
```

## Version

The config file is versioned. At this time, the only supported version is `1`.
It is required to have `version` present in the config file.

## FOSSA communication

| Value                   | Required? | Description                        | Suggested default       |
|-------------------------|-----------|------------------------------------|-------------------------|
| `fossa_endpoint`        | Required  | The address to the FOSSA instance. | `https://app.fossa.com` |
| `fossa_integration_key` | Required  | The API key for FOSSA.             | N/A                     |

FOSSA integration keys can be created at [Settings → Integrations → API](https://app.fossa.com/account/settings/integrations/api_tokens).

The existing level of functionality will always be supported using a "push-only" key,
but future features may require a "full" key to get the most use.

## Debugging

This block specifies where Broker stores its debugging artifacts.
For more information on what a "debugging artifact" is, see [Debug Artifacts](./debug-artifacts.md).

| Value            | Required? | Description                                                | Suggested default                             |
|------------------|-----------|------------------------------------------------------------|-----------------------------------------------|
| `location`       | Required  | The root directory into which debug artifacts are written. | `{USER_HOME}/.config/fossa/broker/debugging/` |
| `retention.days` | Optional  | Remove debug artifacts that are older than this time span. | `7`                                           |

## Integrations

Broker can be configured to integrate with multiple code hosts using this configuration block.
This is an array of blocks, specified by `type`.

Supported types:
| Type  | Description             |
|-------|-------------------------|
| `git` | A remote git repository |

### git

This block specifies how to configure Broker to communicate with a git server for a specific git repository.

| Value           | Required? | Description                                                                                   | Suggested default |
|-----------------|-----------|-----------------------------------------------------------------------------------------------|-------------------|
| `poll_interval` | Required  | How often Broker checks with the remote repository to see whether it has changed.<sup>1</sup> | `1 hour`          |
| `remote`        | Required  | The remote git repository address.                                                            | N/A               |
| `auth`          | Required  | Required authentication to clone this repository.                                             | N/A               |
| `team`          | Optional  | The team in FOSSA to which this project should be assigned.<sup>2</sup>                       | N/A               |
| `title`         | Optional  | Specify a custom title for the project instead of using the default.<sup>3</sup>              | N/A               |

**[1]**: The poll interval defines the interval at which Broker _checks for updates_, not the interval at which Broker actually analyzes the repository.
For more details on authentication, see [integration authentication](#integration-authentication).

**[2]**: Team settings only affect newly imported projects. Changing this value later requires using the FOSSA UI.
If the project already exists before transitioning it to be managed by Broker, this also has no effect.

**[3]**: Title settings only affect newly imported projects. Changing this value later requires using the FOSSA UI.
If the project already exists before transitioning it to be managed by Broker, this also has no effect.
If unspecified, Broker uses a default title, which is just the configured `git` remote.

# Appendix

## `duration` values

A duration is made up of `{value}{unit}` pairs, where `value` is the count of `unit`s.
For example, the input `5h 30min` means "5 hours and 30 minutes".
If a single `value` is provided with no `time unit`, it is assumed to be seconds.

To specify a time unit, use one of the below forms:

- Nanoseconds: `nsec`, `ns`
- Microseconds: `usec`, `us`
- Milliseconds: `msec`, `ms`
- Seconds: `seconds`, `second`, `sec`, `s`
- Minutes: `minutes`, `minute`, `min`, `m`
- Hours: `hours`, `hour`, `hr`, `h`
- Days: `days`, `day`, `d`
- Weeks: `weeks`, `week`, `w`
- Months: `months`, `month`, `M`
- Years: `years`, `year`, `y`

Examples for valid durations:

| Input                | Description                              |
|----------------------|------------------------------------------|
| `2h`                 | 2 hours                                  |
| `2hours`             | 2 hours                                  |
| `48hr`               | 48 hours                                 |
| `1y 12month`         | 1 year and 12 months                     |
| `55s 500ms`          | 55 seconds and 500 milliseconds          |
| `300ms 20s 5day`     | 5 days, 20 seconds, and 300 milliseconds |
| `5day 4hours 10days` | 15 days and 4 hours                      |

## Integration authentication

Integrations support several possible authentication schemes, specified by `type`.
Which authentication method used mostly depends on your specific git server and the URL provided in the integration.

If the `url` begins with `http://` or `https://`, valid authentication types are `http_basic` or `http_header`.
If the `url` begins with `ssh://`, valid authentication types are `ssh_key` or `ssh_key_file`.

**Security:** Broker assumes the local file system is trusted.
While it does its best to ensure secrets exist on disk for the minimum time possible, it may write secrets to the temporary directory during the course of its operation.
On unix-based operating systems, the temporary directory location may be specified with the `TMPDIR` environment variable.

### `none`

If no authentication is required, specify type "none".
This requires a "transport" field, so that Broker can determine which transport mechanism (HTTP or SSH) to use to clone the repository.
Usually this is determined automatically by the authentication type, but in this case it has to be manually specified.

Example integration block:

```yaml
- type: git
  poll_interval: 1h
  remote: https://github.com/fossas/broker.git
  auth:
    type: none
    transport: http
```

### `http_basic`

Performs authentication with a username and password.
Example integration block:

```yaml
- type: git
  poll_interval: 1h
  remote: https://github.com/fossas/broker.git
  auth:
    type: http_basic
    username: jssblck
    password: abcd1234
```

### `http_header`

Performs authentication with a constant header.
Example integration block:

```yaml
- type: git
  poll_interval: 1h
  remote: https://github.com/fossas/broker.git
  auth:
    type: http_header
    header: "Authorization: Bearer abcd1234"
```

### `ssh_key`

Performs authentication with a constant SSH private key.
Example integration block:

```yaml
- type: git
  poll_interval: 1h
  remote: git@github.com:fossas/broker.git
  auth:
    type: ssh_key
    key: |
      -----BEGIN OPENSSH PRIVATE KEY-----
      key goes here
      -----END OPENSSH PRIVATE KEY-----
```

### `ssh_key_file`

Performs authentication with an SSH private key file.
Example integration block:

```yaml
- type: git
  poll_interval: 1h
  remote: git@github.com:fossas/broker.git
  auth:
    type: ssh_key_file
    path: /home/me/.ssh/id_rsa
```
