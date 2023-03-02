use error_stack::fmt::HookContext;
use error_stack::{IntoReport, Report, Result, ResultExt};
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const DOCKERFILE: &str = "Dockerfile";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("cargo env var missing: '{0}'")]
    CargoEnv(String),

    #[error("write file at '{0}'")]
    WriteFile(PathBuf),

    #[error("generate cargo build variables")]
    CargoBuildVars,
}

fn main() -> Result<(), Error> {
    Report::set_color_mode(error_stack::fmt::ColorMode::Color);
    Report::install_debug_hook(Help::debug_hook);

    // Read build info and output it as env vars for later.
    generate_cargo_build_vars()?;

    // TODO: Use `version` to generate installers.
    let version = env_var("CARGO_PKG_VERSION")?;
    let root = PathBuf::from(env_var("CARGO_MANIFEST_DIR")?);

    // TODO: Generate installers, e.g. `install-latest.sh` and `install-latest.ps1`.
    write_file(&root, DOCKERFILE, generate_dockerfile(&version))?;

    // Only need to re-generate this if the manifest changes,
    // because the only thing this script depends on is the version.
    //
    // Disabled for now so that `generate_cargo_build_vars` always has latest data.
    // (Running without emitting a 'cargo:rerun-if-changed' directive causes default change detection:
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#change-detection)
    // TODO: make this smarter, as in https://docs.rs/vergen/3.1.0/vergen/fn.generate_cargo_keys.html
    //
    // println!("cargo:rerun-if-changed=Cargo.toml");

    Ok(())
}

fn env_var(var: &str) -> Result<String, Error> {
    env::var(var)
        .into_report()
        .change_context_lazy(|| Error::CargoEnv(var.to_owned()))
        .attach(Help("ensure that this program is running in a Cargo build"))
}

fn write_file<F: FnOnce() -> String>(root: &Path, name: &str, generator: F) -> Result<(), Error> {
    let target = root.join(name);
    fs::write(&target, generator())
        .into_report()
        .change_context_lazy(|| Error::WriteFile(target))
        .attach(Help(
            "ensure that the build is not running on a read-only file system",
        ))
}

fn generate_dockerfile(_version: &str) -> impl FnOnce() -> String {
    move || {
        // TODO: In the future, we should download the already-built binaries from the release
        //       instead of rebuilding them here.
        //       The currently unused version specifier will be useful for this.
        String::from(
            r#"# Generated by build.rs
FROM rust:slim-bullseye as builder

WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim AS runtime

WORKDIR /broker
COPY --from=builder /build/target/release/broker /usr/local/bin

ENTRYPOINT ["/usr/local/bin/broker"]
"#,
        )
    }
}

/// Side-effectful function that inspects the repo state and generates vars used elsewhere in the build.
/// Most notably, this makes the current git sha available at build time, so other parts of the program
/// may reference it with `env!()`. This is used for doc links and diagnostics information.
///
/// https://docs.rs/vergen/latest/vergen/index.html
fn generate_cargo_build_vars() -> Result<(), Error> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .into_report()
        .change_context(Error::CargoBuildVars)?;

    let git_hash = String::from_utf8(output.stdout)
        .into_report()
        .change_context(Error::CargoBuildVars)?;

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    Ok(())
}

/// Provide help text for a given error.
struct Help(&'static str);

impl Help {
    /// Prints the help text and attaches it to the error context stack.
    fn debug_hook(Help(content): &Self, context: &mut HookContext<Self>) {
        context.push_body(format!("help: {content}"));
    }
}
