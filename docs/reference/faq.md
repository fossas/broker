
# Frequently Asked Questions

We strive to have most questions answerable by the docs generally, but some questions
aren't obviously answered by documentation (often because they span multiple topics).

This is the home for such questions! If you have a question not answered here,
feel free to send us a support request via [support.fossa.com](https://support.fossa.com).

## General questions

### Where is the data root for Broker?

- On Linux and macOS: `~/.config/fossa/broker/`
- On Windows: `%USERPROFILE%\.config\fossa\broker`

Elsewhere in these docs we refer to this as though it is an environment variable, as `DATA_ROOT`.
Note that this is not a true environment variable, we just use it this way inside file paths in the documentation
to make relative file paths clear.

Most Broker subcommands allow customizing the data root via the `-r` flag.
For more information on this and other runtime customization, run `broker -h`.

### Can I customize the temporary directory used by Broker?

- On Linux and macOS: set the `TMPDIR` environment variable.
- On Windows: Broker uses the `GetTempPath` system call,
  [which checks for the existence of environment variables in the following order](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-gettemppath2a#remarks)
  and uses the first path found:
  - The path specified by the `TMP` environment variable.
  - The path specified by the `TEMP` environment variable.
  - The path specified by the `USERPROFILE` environment variable.
  - The Windows directory.

### Where is the config file stored?

- On macOS and Linux, the config is stored at `$DATA_ROOT/config.yml`.
- On Windows, the config is stored at `%DATA_ROOT%\config.yml`.

Most Broker subcommands allow customizing the config location independent of the data root via the `-c` flag.
For more information on this and other runtime customization, run `broker -h`.

### Where is the local database stored?

- On macOS and Linux, the database is stored at `$DATA_ROOT/db.sqlite`.
- On Windows, the database is stored at `%DATA_ROOT%\db.sqlite`.

The database may consist of multiple files with the `db.sqlite` prefix, e.g. `db.sqlite-shm` or `db.sqlite-wal`.
These files are an implementation detail of how Broker accesses the database. 
If you are moving or deleting the database, ensure that these files are moved or deleted as well.

The database may be deleted at any time so long as Broker is not currently running.
Note that doing so may cause references in an integration that have already been scanned and uploaded to FOSSA
to be scanned and uploaded again.

Most Broker subcommands allow customizing the database location independent of the data root via the `-d` flag.
For more information on this and other runtime customization, run `broker -h`.

### Where is the local task queue stored?

- On macOS and Linux, the queue is stored at `$DATA_ROOT/broker-queue/{queue name}`.
- On Windows, the queue is stored at `%DATA_ROOT%\broker-queue\{queue name}`.

The local task queue can be deleted at any time so long as Broker is not currently running.
Note that doing so may cause rework: if Broker has already scanned an integration but not yet uploaded it, 
and the task queue has been deleted, Broker will need to rescan that integration before it can upload it.

### Where are debug artifacts stored?

Debug artifacts are stored at the `debugging.location` configured in the [config file](./config.md#debugging).

For more information about debug artifacts, see the [debug artifact reference](./debug-artifacts.md).<br>
To learn how to create a debug bundle containing these artifacts for FOSSA Support, see [`broker fix`](../subcommands/fix.md).<br>
To learn what is contained in a debug bundle, see the [debug bundle reference](./debug-bundle.md).

### Where does Broker store the downloaded FOSSA CLI?

FOSSA CLI is downloaded to `$DATA_ROOT/fossa`.

## `broker run`

### Does Broker understand FOSSA CLI config files checked into the repository being scanned?

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

### What is scanned from a `git` integration during `broker run`?

For `git` integrations, Broker considers the following to be a "reference":

- Any tag
- Any branch `HEAD` commit

Broker first enumerates all references in the git repository.

_Note that tags cannot be modified; once a tag has been created to "modify" it in `git` requires that the tag is_
_deleted and then created with the same name. Such modifications are actually creation of the tag,_
_and as such any tag that was "modified" since the last scan is re-canned by Broker._

After enumerating the list of references, Broker then uses its local database to filter any reference that it has already scanned.
Note that this means that a modified tag would then be filtered at this step,
if the previous iteration of that tag had already been scanned by Broker on the local system.

Finally, if Broker then sees no valid references to scan, it logs `No changes to {integration name}` in its output.
This occurs whether there _were_ valid references that were filtered, or whether there were no valid references in the first place.

Broker will then poll the integration on the next configured `poll_interval` and perform this process over again.
