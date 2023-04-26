
# Install Broker: Kubernetes

Coming soon! In order to write these docs we need to first publish Broker to the Github Container Registry
so we can reference actual URLs and values.

In general Broker works in any Docker environment, we just recommend tailoring the environment and config file
such that the [`debug.location` value](./config.md#debugging) references a persistent volume so that it survives pod restarts,
and Broker has the ability to create temporary files and directories on disk.

If a Kubernetes installation guide is something you'd like to see prioritized, please let us know!
Otherwise we'll mention it in the changelogs when we add this.
