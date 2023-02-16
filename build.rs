use error_stack::fmt::HookContext;
use error_stack::{IntoReport, Report, Result, ResultExt};
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

const INSTALLER_BASH: &str = "install-latest.sh";
const INSTALLER_PWRSH: &str = "install-latest.ps1";
const DOCKERFILE: &str = "Dockerfile";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("cargo env var missing: '{0}'")]
    CargoEnv(String),

    #[error("write file at '{0}'")]
    WriteFile(PathBuf),
}

fn main() -> Result<(), Error> {
    Report::set_color_mode(error_stack::fmt::ColorMode::Color);
    Report::install_debug_hook(Help::debug_hook);

    let version = env_var("CARGO_PKG_VERSION")?;
    let root = PathBuf::from(env_var("CARGO_MANIFEST_DIR")?);

    write_file(&root, INSTALLER_BASH, generate_installer_bash(&version))?;
    write_file(&root, INSTALLER_PWRSH, generate_installer_pwrsh(&version))?;
    write_file(&root, DOCKERFILE, generate_dockerfile(&version))?;

    println!("cargo:rerun-if-changed=Cargo.toml");
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

fn generate_installer_bash(version: &str) -> impl FnOnce() -> String {
    let version = version.to_owned();
    move || format!("echo 'Hello bash installer {version}'")
}

fn generate_installer_pwrsh(version: &str) -> impl FnOnce() -> String {
    let version = version.to_owned();
    move || format!("echo 'Hello powershell installer {version}'")
}

fn generate_dockerfile(version: &str) -> impl FnOnce() -> String {
    let version = version.to_owned();
    move || format!("echo 'Hello docker {version}'")
}

/// Provide help text for a given error.
struct Help(&'static str);

impl Help {
    /// Prints the help text and attaches it to the error context stack.
    fn debug_hook(Help(content): &Self, context: &mut HookContext<Self>) {
        context.push_body(format!("help: {content}"));
    }
}
