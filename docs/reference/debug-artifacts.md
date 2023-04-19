# Debug artifacts

"Debug artifacts" refers to the contents of the directory indicated by the [_debugging.location_](./config.md#debugging) config value.

The contents of this directory are:
- Traces from the Broker program, detailing exactly what steps Broker is taking and with what data.
  - Broker **redacts secrets** from trace logs.
  - Broker **does not** include the raw contents of project source code in trace logs.
- Debug bundles collected from running [FOSSA CLI](https://github.com/fossas/fossa-cli) on your projects.

These debug artifacts are available for users to view at any time, and are most commonly accessed by
collecting a [debug bundle](./debug-bundle.md) and sending that to FOSSA Support.
