## v0.3.1

Features:
- Remove 30 day limit restriction for integration scanning

## v0.3.0

Features:
- Broker is able to toggle if users want to scan branches, as well as being able to target specific branches to scan. Broker is also 
  able to toggle scanning tags.
- Broker checks for early network misconfigurations with preflight checks
- Broker fix surfaces failing integration scans
- Broker now does not fail fatally if `fossa analyze` returns an error, and instead just reports the errors as a warning
- Broker now detects unknown fields in config and returns errors

## v0.2.3

Bug fix:

- Broker was not properly noting which revisions it had scanned, so it was scanning all recent tags and branches on every poll cycle. This is now fixed.

## v0.2.2

Bug fix:

- Broker now copies its debug bundle (generated during `broker fix`) from the system temp location instead of renaming.
  This resolves issues preventing debug bundles from being stored for Linux installations where the temporary location
  and the home folder are on separate mount points.

## v0.2.1

Features:
- Broker for Linux is now statically built.
  This means Broker should no longer rely on dynamic dependencies such as `libc` on the target system.
- Broker now reports to FOSSA that the build is submitted by Broker instead of FOSSA CLI.
  Today FOSSA doesn't do anything with this information,
  but in the future we plan to use this to display projects that are imported by Broker with a different icon or different search parameters.

Bug fixes:
- Copy debug bundles instead of renaming them from the temp location.
  This resolves issues preventing debug bundles from being stored for Linux installations where the temporary location
  and the home folder are on separate mount points.
- Locate `fossa` in `PATH` before running it.
  This resolves issues where some Linux implementations cannot execute commands without the full path.

## v0.2.0

Adds debug bundle generation to `broker fix`.
Debug bundles are generated automatically if `broker fix` finds errors, or can be generated via `broker fix --export-bundle`.

For more information see the [`fix` subcommand documentation](https://github.com/fossas/broker/blob/main/docs/subcommands/fix.md)
and the [debug bundle reference](https://github.com/fossas/broker/blob/main/docs/reference/debug-bundle.md).

Adds support for assigning newly imported projects to a team in FOSSA and setting their title.
For more information, see the [config reference](https://github.com/fossas/broker/blob/main/docs/reference/config.md#integrations).

## v0.1.1

Add `broker fix`

`broker fix` tests your connection to FOSSA and your `git` integrations and gives you advice on how to fix any problems it encounters.

For more information on `broker fix`, see the [`broker fix` subcommand documentation](https://github.com/fossas/broker/blob/main/docs/subcommands/fix.md).

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

See the [Broker User Manual](https://github.com/fossas/broker/blob/main/docs/README.md) for more help using Broker.
