
# Frequently Asked Questions

We strive to have most questions answerable by the docs generally, but some questions
aren't obviously answered by documentation (often because they span multiple topics).

This is the home for such questions! If you have a question not answered here,
feel free to open an issue or send us a support request.

## Does Broker understand FOSSA CLI config files checked into the repository being scanned?

_This question refers to FOSSA CLI's [`.fossa.yml` config file](https://github.com/fossas/fossa-cli/blob/master/docs/references/files/fossa-yml.md)_
_and FOSSA CLI's [`fossa-deps` config file](https://github.com/fossas/fossa-cli/blob/master/docs/references/files/fossa-deps.md)._

Broker itself does not. However, when Broker runs FOSSA CLI, the CLI does read those config files, so they are respected.
The main caveat is that FOSSA CLI does not actually upload the results of the scan to FOSSA; Broker does.

This is done for a few reasons, but primarily in order to offer predictability and observability to the upload process.
This is useful both for FOSSA and for IT organizations, since this allows Broker to report extremely comprehensive tracing of low-level details
in its debugging information.

The upshot of this is that any `.fossa.yml` config file setting that controls how the project information is uploaded to FOSSA
does not take effect. As of the last time we reviewed, this means the following top level fields of `.fossa.yml` are ignored
when the project is ingested with Broker:

```
server
apiKey
project (and all children)
revision (and all children)
```

A goal of Broker is to have a similar set of settings available in [Broker's config file](./config.md).
If something is missing and you need it to be there, send us a support request!
