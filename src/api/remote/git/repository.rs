//! Wrapper for Git
use base64::{engine::general_purpose, Engine as _};
use error_stack::{bail, report, Report};
use itertools::Itertools;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, NamedTempFile, TempDir};
use thiserror::Error;

use super::Reference;
use crate::ext::command::{Command, CommandDescriber, Output, OutputProvider, Value};
use crate::ext::error_stack::{ErrorHelper, IntoContext};
use crate::ext::result::WrapOk;
use crate::{api::http, api::remote::git, api::ssh, ext::error_stack::DescribeContext};

use super::transport::Transport;

/// Errors encountered during the clone.
#[derive(Debug, Error)]
pub enum Error {
    /// This module shells out to git, and that failed.
    #[error("run command: {}", str::trim(.0))]
    Execution(String),

    /// Creating a temporary directory failed.
    #[error("create temporary directory in system temp location: {}", .0.display())]
    TempDirCreation(PathBuf),

    /// When git perform SSH authentication, this module needs to create a file to hold the key.
    #[error("create temporary ssh key file")]
    SshKeyFileCreation,

    /// Parsing git output failed.
    #[error("parse git output")]
    ParseGitOutput,

    /// When we set up a clone to use HTTP, if the user has erroneously provided an SSH remote,
    /// the clone will silently use the SSH configuration on the user's local machine.
    ///
    /// This is because we cannot configure SSH to "nothing", else we break the clone:
    /// it is not valid to configure git to use a "custom ssh command" which then does not provide SSH authentication.
    ///
    /// Given this, prior to using a remote to perform an HTTP clone this module checks whether
    /// the remote address begins with the literal `http` as a very simple form of validation.
    /// If it does not, this error occurs.
    #[error("http remote '{0}' does not begin with 'http'")]
    HttpRemoteInvalid(String),

    /// It's possible, although unlikely, that a path on the file system is not a valid UTF8 string.
    /// If this occurs when creating the temporary path to which the directory is cloned,
    /// this module cannot provide that path as an argument to the git executable and this error is returned.
    #[error("path on local system is not a valid UTF8 string: {0}")]
    PathNotValidUtf8(PathBuf),
}

impl Error {
    fn running_git_command<D: CommandDescriber>(describer: D) -> Self {
        Self::Execution(describer.describe().to_string())
    }

    fn creating_temp_dir() -> Self {
        Error::TempDirCreation(env::temp_dir())
    }

    fn path_invalid_utf(path: &Path) -> Self {
        Error::PathNotValidUtf8(path.to_path_buf())
    }
}

/// List references that have been updated
#[tracing::instrument]
pub async fn list_references(transport: &Transport) -> Result<Vec<Reference>, Report<Error>> {
    get_all_references(transport).await
}

/// Clone a [`Reference`] into a temporary directory.
#[tracing::instrument]
pub async fn clone_reference(
    transport: &Transport,
    reference: &Reference,
) -> Result<TempDir, Report<Error>> {
    blobless_clone(transport, Some(reference)).await
}

/// The args for the call to ls-remote
fn ls_remote_args(transport: &Transport) -> Vec<Value> {
    vec![
        Value::new_plain("ls-remote"),
        Value::new_plain("--quiet"),
        Value::new_plain(transport.endpoint().to_string().as_str()),
    ]
}

/// ls_remote calls `git ls-remote <endpoint>` on the transport's endpoint
#[tracing::instrument(skip(transport))]
pub async fn ls_remote(transport: &Transport) -> Result<String, Report<Error>> {
    let output = run_git(transport, &ls_remote_args(transport), None).await?;
    let output = String::from_utf8(output.stdout()).context(Error::ParseGitOutput)?;
    Ok(output)
}

#[tracing::instrument(skip(transport))]
async fn get_all_references(transport: &Transport) -> Result<Vec<Reference>, Report<Error>> {
    let output = ls_remote(transport).await?;
    let references = parse_ls_remote(output);

    // Tags sometimes get duplicated in the output from `git ls-remote`, like this:
    // b72eb52c09df108c81e755bc3a083ce56d7e4197        refs/tags/v0.0.1
    // ffb878b5eb456e7e1725606192765dcb6c7e78b8        refs/tags/v0.0.1^{}
    //
    // We can use either of these (the commit resolves to the ^{} version when we check it out), but we need to
    // de-dupe it
    references.into_iter().unique().collect_vec().wrap_ok()
}

/// Construct a git command, including the default args and the environment required for the transport's auth
#[tracing::instrument(skip(transport))]
fn construct_git_command(
    transport: &Transport,
    args: &[Value],
    cwd: Option<&Path>,
) -> Result<Command, Report<Error>> {
    let args = default_args(transport)?
        .into_iter()
        .chain(args.iter().cloned().map_into())
        .collect::<Vec<_>>();

    let mut ssh_key_file = NamedTempFile::new()
        .context(Error::SshKeyFileCreation)
        .describe("Broker must create a temporary SSH key file (even if not using SSH key authentication) to ensure reproducible authentication")?;
    let env = env_vars(transport, &mut ssh_key_file)?;

    let mut command = Command::new("git")
        .args(args)
        .envs(env)
        .env_remove("GIT_ASKPASS");
    if let Some(directory) = cwd {
        command = command.current_dir(directory);
    }
    Ok(command)
}

#[tracing::instrument(skip(transport))]
async fn run_git(
    transport: &Transport,
    args: &[Value],
    cwd: Option<&Path>,
) -> Result<Output, Report<Error>> {
    let command = construct_git_command(transport, args, cwd)?;
    let output = command
        .output()
        .await
        .context_lazy(|| Error::running_git_command(&command))?;

    if !output.status().success() {
        bail!(Error::running_git_command(&output));
    }

    Ok(output)
}

/// Construct a pastable string containing a git command, including the default args and the environment required for the transport's auth
#[tracing::instrument(skip(transport))]
fn pastable_git_command(
    transport: &Transport,
    args: &[Value],
    cwd: Option<&Path>,
) -> Result<String, Report<Error>> {
    let command = construct_git_command(transport, args, cwd)?;
    command.describe().pastable().wrap_ok()
}

/// Construct a pastable string containing a `git ls-remote` command, including the default args and the environment required for the transport's auth
pub fn pastable_ls_remote_command(transport: &Transport) -> Result<String, Report<Error>> {
    pastable_git_command(transport, &ls_remote_args(transport), None)
}

/// Do a blobless clone of the repository, checking out the Reference if it exists
#[tracing::instrument(skip(transport))]
async fn blobless_clone(
    transport: &Transport,
    reference: Option<&Reference>,
) -> Result<TempDir, Report<Error>> {
    let mut args = vec![
        Value::new_plain("clone"),
        Value::new_plain("--filter=blob:none"),
    ];

    if let Some(reference) = reference {
        args.push(Value::new_plain("--branch"));
        args.push(Value::new_plain(reference.name()));
    }

    let endpoint = transport.endpoint().to_string();
    let tmpdir = tempdir()
        .context_lazy(Error::creating_temp_dir)
        .help("altering the temporary directory location may resolve this issue")
        .describe("temporary directory location uses $TMPDIR on Linux and macOS; for Windows it uses the 'GetTempPath' system call")?;

    let tmp_path = tmpdir
        .path()
        .to_str()
        .ok_or_else(|| report!(Error::path_invalid_utf(tmpdir.path())))
        .help("changing the system temporary directory to a path that is valid UTF-8 may resolve this issue")
        .describe("Broker needs the temporary path to be valid UTF-8 because it's sent as an argument to the git executable")?;

    args.push(Value::new_plain(&endpoint));
    args.push(Value::new_plain(tmp_path));
    run_git(transport, args.as_slice(), None)
        .await
        .map(|_| tmpdir)
}

#[tracing::instrument(skip(transport))]
fn default_args(transport: &Transport) -> Result<Vec<Value>, Report<Error>> {
    if let Transport::Http { endpoint, .. } = transport {
        if !endpoint.starts_with("http") {
            bail!(Error::HttpRemoteInvalid(endpoint.to_string()));
        }
    }

    let header_args = match transport.auth() {
        // git \
        //   -c credential-helper="" \
        //   -c http.extraHeader="AUTHORIZATION: Basic ${B64_GITHUB_TOKEN}" \
        //   clone https://github.com/spatten/fanopticon
        git::transport::Auth::Http(Some(http::Auth::Basic { username, password })) => {
            let secret_header = format!("{}:{}", username, password.expose_secret());
            let secret_header = general_purpose::STANDARD.encode(secret_header);

            vec![
                Value::new_plain("-c"),
                Value::format_secret(
                    "http.extraHeader=AUTHORIZATION: Basic {secret}",
                    secret_header,
                ),
            ]
        }
        git::transport::Auth::Http(Some(http::Auth::Header(header))) => {
            vec![
                Value::new_plain("-c"),
                Value::format_secret("http.extraHeader={secret}", header),
            ]
        }
        _ => vec![],
    };

    // Credential helpers can override the header provided by http.extraHeader,
    // so we need to get rid of them by setting `credential-helper` to an empty value.
    vec!["-c", "credential.helper="]
        .into_iter()
        .map(Value::new_plain)
        .chain(header_args.into_iter())
        .collect_vec()
        .wrap_ok()
}

#[tracing::instrument(skip(transport))]
fn env_vars(
    transport: &Transport,
    ssh_key_file: &mut NamedTempFile<File>,
) -> Result<Vec<(String, Value)>, Report<Error>> {
    let s = String::from;

    let custom_command = match transport.auth() {
        git::transport::Auth::Ssh(ssh::Auth::KeyFile(path)) => {
            // The key is secret, its location on disk is not.
            let command = git_ssh_command(&path)?;
            vec![(s("GIT_SSH_COMMAND"), Value::new_plain(command))]
        }
        git::transport::Auth::Ssh(ssh::Auth::KeyValue(key)) => {
            // Write the contents of the SSH key to a file so that we can point to it.
            ssh_key_file
                .write_all(key.expose_secret().as_bytes())
                .context(Error::SshKeyFileCreation)?;

            // The key is secret, its location on disk is not.
            let command = git_ssh_command(ssh_key_file.path())?;
            vec![(s("GIT_SSH_COMMAND"), Value::new_plain(command))]
        }
        _ => vec![],
    };

    // Turn off terminal prompts if auth fails.
    let disable_prompt = vec![
        (s("GIT_TERMINAL_PROMPT"), Value::new_plain("0")),
        (s("GCM_INTERACTIVE"), Value::new_plain("never")),
    ];

    disable_prompt
        .into_iter()
        .chain(custom_command)
        .collect_vec()
        .wrap_ok()
}

// git_ssh_command is passed into the GIT_SSH_COMMAND env variable. This makes git use this command
// when it tries to make an SSH connection.
// "-o IdentitiesOnly=yes" means "only use the identity file pointed to by the -i arg"
// "-o StrictHostKeyChecking=no" avoids errors when the host is not in ssh's knownHosts file
// "-F /dev/null" means "start with an empty ssh config"
#[tracing::instrument]
fn git_ssh_command(path: &Path) -> Result<String, Report<Error>> {
    path.to_str()
        .ok_or_else(|| report!(Error::PathNotValidUtf8(path.to_path_buf())))
        .describe("Broker requires that the path to the SSH key is valid UTF-8 because it's passed as an argument to the git executable")
        .map(|path| {
            format!("ssh -i {path} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -F /dev/null")
        })
}

/// parse the output from `git ls-remote --quiet`
/// The output will look something like this:
///
/// git ls-remote --quiet
/// 9e9834e875bcc07745495b05fe7e73d85d8962b9        HEAD
/// 55aa4f2fc908b42aa1d3e958d115a32a55126a73        refs/heads/add-git-wrapper
/// bb6d81b3591502b87d77e9ee3e32ad741cb8fa53        refs/heads/async-work-queue
/// 9e9834e875bcc07745495b05fe7e73d85d8962b9        refs/heads/main
/// dc575604056303cb16131c1e468077392470b1a6        refs/heads/tracing-logging
/// cb234292277e66703399bf90c841cdffd42db1cf        refs/heads/tracing-retention
/// 038f86242d02089bb3c7c0fd8c408e624de9f664        refs/pull/1/head
/// 55aa4f2fc908b42aa1d3e958d115a32a55126a73        refs/pull/10/head
/// a79998dec9c9732c9f5e49767e1064ebc2375089        refs/pull/10/merge
/// dc575604056303cb16131c1e468077392470b1a6        refs/pull/11/head
/// 6845f2dc4db87996d7b22b586357bd7513d50803        refs/pull/11/merge
/// b72eb52c09df108c81e755bc3a083ce56d7e4197        refs/tags/v0.0.1
/// ffb878b5eb456e7e1725606192765dcb6c7e78b8        refs/tags/v0.0.1^{}
///
/// We only want the branches (which start with `refs/head/` and the tags (which start with `refs/tags`))
/// Tags that end in ^{} should have the ^{} stripped from them. This will usually end up with a duplicate, so we
/// de-dupe before returning
#[tracing::instrument(skip_all)]
fn parse_ls_remote(output: String) -> Vec<Reference> {
    output
        .split('\n')
        .map(line_to_git_ref)
        .filter(|r| r.is_some())
        .flatten()
        .collect()
}

#[tracing::instrument]
fn line_to_git_ref(line: &str) -> Option<Reference> {
    let mut parsed = line.split_whitespace();
    let commit = parsed.next()?;
    let commit = String::from(commit);
    let reference = parsed.next()?;
    if let Some(tag) = reference.strip_prefix("refs/tags/") {
        let tag = tag.strip_suffix("^{}").unwrap_or(tag);
        Some(Reference::new_tag(tag.to_string(), commit))
    } else {
        reference
            .strip_prefix("refs/heads/")
            .map(|branch| Reference::new_branch(branch.to_string(), commit))
    }
}
