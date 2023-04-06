
# Install Broker: Local System

Broker doesn't currently have an installation script, so this is a bit of a manual process.

First, navigate to the latest release by clicking this link: https://github.com/fossas/broker/releases/latest

The release will have a table under `Download`, with an entry for each system Broker supports.
Choose the appropriate download based on your local system:

- For Windows, choose `x86_64-pc-windows-msvc`. Download the `.zip` file, not the `.pdb` file (the latter is for debugging).
- For macOS:
  - If you have an M-series processor, choose `aarch64-apple-darwin`.
  - If you have an Intel processor or you aren't sure, choose `x86_64-apple-darwin`.
- For Linux, choose `x86_64-unknown-linux-gnu`.

Once you have that downloaded, open the archive, usually by double clicking it (although you can also use the command line).
Inside the archive you'll find the `LICENSE` file, a copy of the `README`, and the `broker` executable (`broker.exe` on Windows).

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