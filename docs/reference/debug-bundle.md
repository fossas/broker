# Reference: Debug bundle

The Broker debug bundle contains all the information required for FOSSA to troubleshoot Broker in your environment.

Specifically, when collecting a debug bundle, Broker includes all the file contents of the directory indicated by the
[_debugging.location_](./config.md#debugging) config value.

The contents of this directory are:
- Traces from the Broker program, detailing exactly what steps Broker is taking and with what data.
  - Broker **redacts secrets** from trace logs.
  - Broker **does not** include the raw contents of project source code in trace logs.
- Debug bundles collected from running [FOSSA CLI](https://github.com/fossas/fossa-cli) on your projects.

The same information that Broker collects in the debug bundle is available for users to peruse at any time,
and we highly recommend users double check the debug bundle before sending to ensure proper redaction.
