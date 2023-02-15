
# development

Tags denote releases.
Any commit merged to `main` is expected to be release ready, 
with the exception of the `version` in `Cargo.toml`.
For more detail, see the [release process](#release-process).

Broker follows [semver](https://semver.org/):
- MAJOR version indicates a user facing breaking change.
- MINOR version indicates backwards compatible functionality improvement.
- PATCH version indicates backwards compatible bug fixes.

The initial beta releases of Broker use `0` as the major version; when this changes to `1`
it will not necessarily indicate a breaking change, but future major version increases will.

## compatibility

Broker:
- Tracks the latest version of the Rust compiler and associated tooling at all times.
- Tracks the latest Rust language edition.
- Aggressively upgrades dependencies. We rely on testing to validate our dependencies work.

## setting up your development environment

We recommend Visual Studio Code with the `rust-analyzer` extension.
Install Rust here: https://www.rust-lang.org/tools/install

For any contributors, we recommend the following tools, although they're not required:
```
cargo edit    # https://lib.rs/crates/cargo-edit
cargo nextest # https://nexte.st/
cargo upgrade # https://lib.rs/crates/cargo-upgrades
```

If you're a FOSSA employee who'll be performing releases, we use `cargo-dist` and `cargo-release`:
```
cargo dist     # https://github.com/axodotdev/cargo-dist
cargo release  # https://github.com/crate-ci/cargo-release
```

Our release process is generated with `cargo dist`.
To regenerate it, run:
```
cargo dist generate-ci github \
  --installer github-powershell \
  --installer github-shell
```

## style guide

Make your code look like the code around it. Consistency is the name of the game.

You should submit changes to this doc if you think you can improve it,
or if a case should be covered by this doc, but currently is not.

Use `rustfmt` for formatting.
Our CI setup enforces that all changes pass a `rustfmt` run with no differences.

Our CI systems ensure that all patches pass `clippy` checks.

Comments should describe the "why", type signatures should describe the "what", and the code should describe the "how".

We use the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/about.html)
during code review; if you want to get ahead of the curve check it out!

Ideally, every PR should check for updated dependencies and update them if applicable;
if this is not realistic at minimum every non-bugfix release **must** ensure dependencies are up to date.

## release process

While in a pre-release state, the `version` field in `Cargo.toml` displays at least the
minimal next version with a pre-release indicator.

For example, if version `0.1.0` was just released, the next merge into `main` must set `version` to
at least `0.1.1-pre`. If the next release is known to be a specific version, that's okay to use as well:
for example if we know `0.2.0` is the next planned release, we can set it to `0.2.0-pre`.

When the final commit for that version is merged, it must ensure `version` is accurate.
Reusing the previous example, after a slew of PRs, the final one would set `version` to `0.2.0`.

After this commit is merged to `main`, push a tag (recommended: using `cargo release tag`)
matching the `version` field, with a `v` prefix.

**It is recommended** to instead use `cargo release`, which automates much of this process and has
some safety checks (for example it ensures you're tagging on `main`):

```
cargo release tag     # Review the planned actions
cargo release tag -x  # Execute the planned actions
```

If instead you wish to do this manually, this is an example of what to do:

```
git checkout main  # Ensure you're on main
git pull           # Ensure you're tagging the latest commit
git tag v0.2.0     # Validate this is correct, and don't forget the `v`
git push --tags    # Push the new tag to the remote.
```
