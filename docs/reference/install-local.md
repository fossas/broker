
# Install Broker: Local System

Broker doesn't currently have an installation script, so this is a bit of a manual process.

First, navigate to the latest release by clicking this link: https://github.com/fossas/broker/releases/latest

The release will have a table under `Download`, with an entry for each system Broker supports.
Choose the appropriate download based on your local system:

- For Windows, choose `broker-{version}-x86_64-windows.exe`.
- For Linux, choose `broker-{version}-x86_64-linux`.
  - This is a static binary, and should work for any modern Linux installation.
- For macOS:
  - If you have an M-series processor, choose `broker-{version}-aarch64-macos`.
  - If you have an Intel processor or aren't sure, choose `broker-{version}-x86_64-macos`.

Open a terminal (macOS/Linux) or a command prompt (Windows) and navigate to the location to which you downloaded and extracted Broker.
From there, you may either run Broker directly:

- Windows: `broker.exe -h`
- macOS/Linux: `./broker -h`

You may want to move Broker to a more permanent location.
For macOS and Linux, we recommend moving Broker to `/usr/local/bin/`:

```
; mv ./broker /usr/local/bin/
; broker -h
```

For Windows, this will depend on your `%PATH%` environment variable.
Windows applications don't generally install themselves to the default `%PATH%` locations since these are typically protected folders;
for this reason it's probably simpler to just use Broker in the location to which it was downloaded, or create a new place to store it
and add that to your system's `%PATH%` variable.

The instructions for this differ based on the Windows version being used and your level of access to your Windows workstation.
If you're not sure how to do this, we recommend you work with your system adminstrator.

## Future steps

FOSSA plans to create automated installers for Broker in the future,
so this involved and manual process is hopefully a temporary situation.
