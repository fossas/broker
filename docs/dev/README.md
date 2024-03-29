
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

_Note: the goal is that most common tasks are in `Makefile`, see that for pointers!_

For any contributors, we recommend the following tools, although they're not required:
```
cargo edit    # https://lib.rs/crates/cargo-edit
cargo nextest # https://nexte.st/
cargo upgrade # https://lib.rs/crates/cargo-upgrades
cargo sqlx    # https://lib.rs/crates/sqlx-cli
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

### static builds

We use `musl` with `jemalloc` for static builds.

For more information, including why we use these particular options,
see the [static binary reference](./reference/static-binary.md).

### cross compilation

Sometimes, especially when debugging a platform build issue, it's useful to "cross compile" the project
from your home operating system to a destination operating system.

Steps to do so are located in the [cross compilation reference](./reference/cross-compile.md).

### snapshot testing

Broker uses [insta](https://docs.rs/insta) to perform snapshot testing of outputs.
Refer to `insta`'s [getting started guide](https://insta.rs/docs/) for optimal usage.

The short version of the workflow is that if you get "snapshot errors" during tests,
run `cargo insta test --review" to review the changes and accept/deny them as intentional.

### migrations

We store migrations in `db/migrations` (this is different than `sqlx`'s default of just `migrations`).

To create a migration, ensure `sqlx-cli` is installed [above](#setting-up-your-development-environment) and run:
```
# use underscores for any spaces in <name>
; cargo sqlx migrate add --source db/migrations -r <name>
```

Then fill out the newly generated `up` and `down` scripts.

Up migrations are automatically applied when Broker boots up, but they can be automatically applied:
```
; cargo sqlx migrate run --source db/migrations
; cargo sqlx migrate revert --source db/migrations
```

Applying up migrations always migrates all the way to current, while reverting does one step at a time:
```
; cargo sqlx migrate run --source db/migrations
Applied 20230313231558/migrate state 1 (202.166µs)
Applied 20230313231721/migrate state 2 (135.833µs)
Applied 20230313231723/migrate state 3 (124.667µs)
; cargo sqlx migrate revert --source db/migrations
Applied 20230313231723/revert state 3 (851.083µs)
; cargo sqlx migrate revert --source db/migrations
Applied 20230313231721/revert state 2 (593.584µs)
; cargo sqlx migrate revert --source db/migrations
Applied 20230313231558/revert state (661.875µs)
; cargo sqlx migrate revert --source db/migrations
No migrations available to revert
```

To see migration state, run:
```
; cargo sqlx migrate info --source db/migrations
20230313231558/pending state
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

In the future, the plan is to automate the release process with `make release`.

**It is recommended** to instead use `cargo release` (`cargo install cargo-release` for installation), which automates much of this process and has
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

## smart imports

Broker provides configurable branch/tag scanning for every integration. You can customize your scans 
through these fields listed in the integrations section of your config.yml:

```
integrations:    
  - type: git
    import_branches: true  # Defaults to true
    watched_branches:      # If unspecified, Broker will try to set to main or master if present
      - main  
      - release*             
    import_tags: false     # Defaults to false
```

### default values

If these fields are not set, `import_branches` will be set to `true`, `import_tags` will be set to `false`, and Broker 
will make a best effort approach to set `watched_branches` to `main` or `master` if it is present in the remote.

### branch scanning

In order to scan specific branches, `import_branches` must be set to `true` and the list of branches you intend to scan should be provided under `watched_branches`. Having `watched_branches` set while having `import_branches` set to `false` is an invalid 
combination and will cause Broker to throw errors. 

[Glob matching](https://en.wikipedia.org/wiki/Glob_(programming)) is also provided with your branches. If one of your watched_branches is `release*` and your remote contains branches `release1`, `release2`, and `release-3`. Then all three 
of those branches will be scanned due to glob matching.

### tag scanning

In order to allow Broker to scan tags in your remote, `import_tags` must be set to `true`

### toggling fields

Toggling `import_branches` from `true` to `false` will remove all existing uploaded scans for ALL branches of that particular remote in your local database (this does NOT delete your scans in the FOSSA UI). If toggled from `false` to `true`, Broker will perform as if it is scanning the listed `watched_branches` for the first time. On subsequent poll cycles, Broker will import the latest changes from your configured branches since the last revision (skipping any intermediate commits).

Toggling `import_tags` from `true` to `false` will remove all existing uploaded scans for ALL tags of that particular remote in your local database (this does NOT delete your scans in the FOSSA UI). If toggled from `false` to `true`, Broker will perform as if it is scanning all the remote's tags for the first time. This would mean that all tags for that remote would be scanned. On subsequent poll cycles, Broker will import all created or changed tags since the last poll cycle.
