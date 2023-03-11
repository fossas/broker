//! Wrapper for Git
use base64::{engine::general_purpose, Engine as _};
use chrono;
use error_stack::{ensure, report, IntoReport, Report, ResultExt};
use itertools::Itertools;
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::{tempdir, NamedTempFile, TempDir};
use thiserror::Error;

use super::Reference;
use crate::ext::error_stack::ErrorHelper;
use crate::{api::http, api::remote::git, api::ssh, ext::error_stack::DescribeContext};

use super::transport::Transport;

/// Errors encountered during the clone.
#[derive(Debug, Error)]
pub enum Error {
    /// This module shells out to git, and that failed.
    #[error("git execution failed")]
    GitExecution,

    /// Creating a temporary directory failed.
    #[error("failed to create temporary directory")]
    TempDirCreation,

    /// When git perform SSH authentication, this module needs to create a file to hold the key.
    #[error("failed to create temporary ssh key file")]
    SshKeyFileCreation,

    /// Parsing git output failed.
    #[error("failed to parse git output")]
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
    #[error("http remote does not begin with 'http'")]
    HttpRemoteInvalid,

    /// It's possible, although unlikely, that a path on the file system is not a valid UTF8 string.
    /// If this occurs when creating the temporary path to which the directory is cloned,
    /// this module cannot provide that path as an argument to the git executable and this error is returned.
    #[error("path on local system is not a valid UTF8 string")]
    PathNotValidUtf8,
}

/// List references that have been updated in the last 30 days.
pub fn list_references(transport: &Transport) -> Result<Vec<Reference>, Report<Error>> {
    let references = get_all_references(transport)?;
    references_that_need_scanning(transport, references)
}

/// Clone a [`Reference`] into a temporary directory.
pub fn clone_reference(
    transport: &Transport,
    reference: &Reference,
) -> Result<TempDir, Report<Error>> {
    let tmpdir = blobless_clone(transport, Some(reference))?;
    Ok(tmpdir)
}

fn get_all_references(transport: &Transport) -> Result<Vec<Reference>, Report<Error>> {
    // First, we need to make a temp directory and run `git init` in it
    let tmpdir = tempdir()
        .into_report()
        .change_context(Error::TempDirCreation)
        .describe_lazy(|| format!("attempted to create temporary dir in {:?}", env::temp_dir()))
        .help("temporary directory location uses $TMPDIR on Linux and macOS; for Windows it uses the 'GetTempPath' system call")?;

    // initialize the repo
    let args = vec!["init"];
    run_git(transport, args, Some(tmpdir.path()))?;
    let endpoint = transport.endpoint().to_string();

    // add the remote
    let args = vec!["remote", "add", "origin", &endpoint[..]];
    run_git(transport, args, Some(tmpdir.path()))?;

    // Now that we have an initialized repo, we can get our references with `git ls-remote`
    let args = vec!["ls-remote", "--quiet"];

    let output = run_git(transport, args, Some(tmpdir.path()))?;
    let output = String::from_utf8(output.stdout)
        .into_report()
        .describe("reading output of 'git ls-remote --quiet'")
        .change_context(Error::ParseGitOutput)?;
    let references = parse_ls_remote(output)?;

    // Tags sometimes get duplicated in the output from `git ls-remote`, like this:
    // b72eb52c09df108c81e755bc3a083ce56d7e4197        refs/tags/v0.0.1
    // ffb878b5eb456e7e1725606192765dcb6c7e78b8        refs/tags/v0.0.1^{}
    //
    // We can use either of these (the commit resolves to the ^{} version when we check it out), but we need to
    // de-dupe it
    let references = references.into_iter().unique().collect();
    Ok(references)
}

fn run_git<I, S>(
    transport: &Transport,
    args: I,
    cwd: Option<&Path>,
) -> Result<Output, Report<Error>>
where
    I: IntoIterator<Item = S>,
    String: From<S>,
{
    let mut full_args = default_args(transport)?;
    let args_as_vec: Vec<String> = args.into_iter().map(String::from).collect();
    full_args.append(&mut args_as_vec.clone());

    let mut ssh_key_file = NamedTempFile::new()
        .into_report()
        .change_context(Error::SshKeyFileCreation)
        .describe("creating temp file to write SSH key into in run_git")?;
    let env = env_vars(transport, &mut ssh_key_file)?;

    let mut command = Command::new("git");
    command.args(full_args.clone()).envs(env);
    println!("running git {:?} in directory {:?}", full_args, cwd);
    if let Some(directory) = cwd {
        command.current_dir(directory);
    }
    let output = command
        .output()
        .into_report()
        .change_context(Error::GitExecution)
        .describe_lazy(|| format!("ran git command: {:?}", full_args))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::GitExecution).into_report().describe_lazy(|| {
            format!(
                "running git command {:?}, status was: {}, stderr: {}",
                full_args, output.status, stderr,
            )
        });
    }
    Ok(output)
}

/// Get a list of all branches and tags for the given integration
/// This is done by doing this:
///
/// git init
/// git remote add origin <URL to git repo>
/// git ls-remote --quiet
///
/// and then parsing the results of git ls-remote

/// Filter references by looking at the date of their head commit and only including repos
/// that have been updated in the last 30 days
/// To do this we need a cloned repository so that we can run
/// `git log <some format string that includes that date of the commit> <branch_or_tag_name>`
/// in the cloned repo for each branch or tag
fn references_that_need_scanning(
    transport: &Transport,
    references: Vec<Reference>,
) -> Result<Vec<Reference>, Report<Error>> {
    let tmpdir = blobless_clone(transport, None)
        .describe("cloning into temp directory in references_that_need_scanning")?;

    let initial_len = references.len();
    let filtered_references: Vec<Reference> = references
        .into_iter()
        .filter(|reference| {
            reference_needs_scanning(transport, reference, PathBuf::from(tmpdir.path()))
                .unwrap_or(false)
        })
        .collect();
    println!(
        "there were {} references, and {} of them should be scanned\n{:?}",
        initial_len,
        filtered_references.len(),
        filtered_references,
    );

    Ok(filtered_references)
}

/// A reference needs scanning if its head commit is less than 30 days old
fn reference_needs_scanning(
    transport: &Transport,
    reference: &Reference,
    cloned_repo_dir: PathBuf,
) -> Result<bool, Report<Error>> {
    let reference_string = match reference {
        Reference::Branch { name, .. } => format!("origin/{name}"),
        Reference::Tag { name, .. } => name.clone(),
    };
    let args = vec![
        "log",
        "-n",
        "1",
        "--format=%aI:::%cI",
        &reference_string[..],
    ];
    // git log -n 1 --format="%aI:::%cI" <name of tag or branch>
    // This will return one line containing the author date and committer date separated by ":::". E.g.:
    // The "I" in "aI" and "cI" forces the date to be in strict ISO-8601 format
    //
    // git log -n 1 --format="%ai:::%ci" parse-config-file
    // 2023-02-17 17:14:52 -0800:::2023-02-17 17:14:52 -0800
    //
    // The author and committer dates are almost always the same, but we'll parse both and take the most
    // recent, just to be super safe

    let output = run_git(transport, args, Some(&cloned_repo_dir))?;
    let date_strings = String::from_utf8_lossy(&output.stdout);
    println!(
        "author and committer date for {}: {}",
        reference.name(),
        date_strings
    );
    let mut dates = date_strings.split(":::");
    let earliest_commit_date_that_needs_to_be_scanned =
        chrono::Utc::now() - chrono::Duration::days(30);

    let author_date = dates
        .next()
        .map(|d| d.parse::<chrono::DateTime<chrono::Utc>>());
    if let Some(Ok(author_date)) = author_date {
        if author_date > earliest_commit_date_that_needs_to_be_scanned {
            return Ok(true);
        }
    }

    let committer_date = dates
        .next()
        .map(|d| d.parse::<chrono::DateTime<chrono::Utc>>());
    if let Some(Ok(committer_date)) = committer_date {
        if committer_date > earliest_commit_date_that_needs_to_be_scanned {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Do a blobless clone of the repository, checking out the Reference if it exists
fn blobless_clone(
    transport: &Transport,
    reference: Option<&Reference>,
) -> Result<TempDir, Report<Error>> {
    let tmpdir = tempdir()
        .into_report()
        .change_context(Error::TempDirCreation)
        .describe_lazy(|| format!("attempted to create temporary dir in {:?}", env::temp_dir()))
        .help("temporary directory location uses $TMPDIR on Linux and macOS; for Windows it uses the 'GetTempPath' system call")?;
    let mut args = vec![String::from("clone"), String::from("--filter=blob:none")];
    if let Some(reference) = reference {
        args.append(&mut vec![
            String::from("--branch"),
            reference.name().clone(),
        ]);
    }
    let path = tmpdir
        .path()
        .to_str()
        .ok_or_else(|| report!(Error::PathNotValidUtf8))
        .describe_lazy(|| format!("path provided: {:?}", tmpdir.path()))?;
    args.append(&mut vec![
        transport.endpoint().to_string(),
        path.to_string(),
    ]);
    run_git(transport, args, None)?;
    Ok(tmpdir)
}

fn default_args(transport: &Transport) -> Result<Vec<String>, Report<Error>> {
    ensure!(
        transport.endpoint().starts_with("http"),
        Error::HttpRemoteInvalid,
    );

    let header_args = match transport.auth() {
        // git -c credential-helper="" -c http.extraHeader="AUTHORIZATION: Basic ${B64_GITHUB_TOKEN}" clone https://github.com/spatten/fanopticon
        git::transport::Auth::Http(Some(http::Auth::Basic { username, password })) => {
            let header = format!("{}:{}", username, password.as_ref().expose_secret());
            let base64_header = general_purpose::STANDARD.encode(header);
            let full_header = format!("http.extraHeader=AUTHORIZATION: Basic {}", base64_header);
            vec![String::from("-c"), full_header]
        }
        git::transport::Auth::Http(Some(http::Auth::Header(header))) => {
            let full_header = format!("http.extraHeader={}", header.as_ref().expose_secret());
            vec![String::from("-c"), full_header]
        }
        _ => vec![],
    };

    // Credential helpers can override the header provided by http.extraHeader, so we need to get rid of them by setting `credential-helper` to "".
    let credential_helper_args = vec![String::from("-c"), String::from("credential.helper=")];
    Ok(credential_helper_args
        .into_iter()
        .chain(header_args.into_iter())
        .collect())
}

fn env_vars(
    transport: &Transport,
    ssh_key_file: &mut NamedTempFile<File>,
) -> Result<HashMap<String, String>, Report<Error>> {
    let s = |input| String::from(input);

    let custom_command = match transport.auth() {
        git::transport::Auth::Ssh(ssh::Auth::KeyFile(path)) => {
            vec![(s("GIT_SSH_COMMAND"), git_ssh_command(&path)?)]
        }
        git::transport::Auth::Ssh(ssh::Auth::KeyValue(key)) => {
            // Write the contents of the SSH key to a file so that we can point to it in
            // GIT_SSH_COMMAND
            ssh_key_file
                .write_all(key.as_ref().expose_secret().as_bytes())
                .into_report()
                .change_context(Error::SshKeyFileCreation)?;

            vec![(s("GIT_SSH_COMMAND"), git_ssh_command(ssh_key_file.path())?)]
        }
        _ => vec![],
    };

    // Turn off terminal prompts if auth fails.
    let disable_prompt = vec![
        (s("GIT_TERMINAL_PROMPT"), s("0")),
        (s("GCM_INTERACTIVE"), s("never")),
    ];
    Ok(disable_prompt.into_iter().chain(custom_command).collect())
}

// git_ssh_command is passed into the GIT_SSH_COMMAND env variable. This makes git use this command
// when it tries to make an SSH connection.
// "-o IdentitiesOnly=yes" means "only use the identity file pointed to by the -i arg"
// "-o StrictHostKeyChecking=no" avoids errors when the host is not in ssh's knownHosts file
// "-F /dev/null" means "start with an empty ssh config"
fn git_ssh_command(path: &Path) -> Result<String, Report<Error>> {
    path.to_str()
        .ok_or_else(|| report!(Error::PathNotValidUtf8))
        .describe_lazy(|| format!("path provided: {:?}", path))
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
fn parse_ls_remote(output: String) -> Result<Vec<Reference>, Report<Error>> {
    let results = output
        .split('\n')
        .map(line_to_git_ref)
        .filter(|r| r.is_some())
        .flatten()
        .collect();
    Ok(results)
}

fn line_to_git_ref(line: &str) -> Option<Reference> {
    let mut parsed = line.split_whitespace();
    let commit = parsed.next()?;
    let commit = String::from(commit);
    let reference = parsed.next()?;

    if let Some(tag) = reference.strip_prefix("refs/tags/") {
        let tag = tag.strip_suffix("^{}").unwrap_or(tag);
        Some(Reference::new_tag(tag.to_string(), commit))
    } else if let Some(branch) = reference.strip_prefix("refs/heads/") {
        Some(Reference::new_branch(branch.to_string(), commit))
    } else {
        None
    }
}
