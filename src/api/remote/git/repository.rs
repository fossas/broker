//! Wrapper for Git
use base64::{engine::general_purpose, Engine as _};
use chrono;
use error_stack::{IntoReport, Report, ResultExt};
use itertools::Itertools;
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::{tempdir, NamedTempFile};

use crate::api::remote::{
    self, Commit, Integration, Reference, ReferenceType, RemoteProvider, RemoteProviderError,
    RemoteReference,
};
use crate::{api::http, api::remote::git, api::ssh, ext::error_stack::DescribeContext};

/// A git repository
#[derive(Debug)]
pub struct Repository {
    /// directory is the location on disk where the repository resides or will reside
    pub directory: PathBuf,
    /// integration contains the info that Broker uses to communicate with the git host
    pub integration: Integration,
}

impl RemoteProvider for Repository {
    fn get_references_that_need_scanning(
        integration: &Integration,
    ) -> Result<Vec<RemoteReference>, Report<RemoteProviderError>> {
        let references = Self::get_all_references(integration)?;
        Self::references_that_need_scanning(references, integration)
    }

    fn clone_branch_or_tag(
        integration: &Integration,
        root_dir: PathBuf,
        remote_reference: &RemoteReference,
    ) -> Result<PathBuf, Report<RemoteProviderError>> {
        let mut path = root_dir;
        path.push(remote_reference.reference.as_ref());

        let repo = Repository {
            directory: path.clone(),
            integration: integration.clone(),
        };
        println!(
            "Cloning reference {:?} into path {:?}",
            remote_reference, path
        );
        repo.blobless_clone(Some(&remote_reference.reference))?;
        Ok(path)
    }
}

impl Repository {
    /// Get a list of all branches and tags for the given integration
    /// This is done by doing this:
    ///
    /// git init
    /// git remote add origin <URL to git repo>
    /// git ls-remote --quiet
    ///
    /// and then parsing the results of git ls-remote
    fn get_all_references(
        integration: &Integration,
    ) -> Result<Vec<RemoteReference>, Report<RemoteProviderError>> {
        // First, we need to make a temp directory and run `git init` in it
        let tmpdir = tempdir()
            .into_report()
            .change_context(RemoteProviderError::RunCommand)
            .describe("creating temp directory in get_reference")?;

        let repo = Repository {
            directory: PathBuf::from(tmpdir.path()),
            integration: integration.clone(),
        };
        // initialize the repo
        let args = vec!["init"];
        repo.run_git(args, Some(repo.directory.clone()))?;
        let endpoint = repo.transport().endpoint().to_string();

        // add the remote
        let args = vec!["remote", "add", "origin", &endpoint[..]];
        repo.run_git(args, Some(repo.directory.clone()))?;

        // Now that we have an initialized repo, we can get our references with `git ls-remote`
        let args = vec!["ls-remote", "--quiet"];

        let output = repo.run_git(args, Some(repo.directory.clone()))?;
        let output = String::from_utf8(output.stdout)
            .into_report()
            .describe("reading output of 'git ls-remote --quiet'")
            .change_context(RemoteProviderError::RunCommand)?;
        let references = Self::parse_ls_remote(output)?;

        // Tags sometimes get duplicated in the output from `git ls-remote`, like this:
        // b72eb52c09df108c81e755bc3a083ce56d7e4197        refs/tags/v0.0.1
        // ffb878b5eb456e7e1725606192765dcb6c7e78b8        refs/tags/v0.0.1^{}
        //
        // We can use either of these (the commit resolves to the ^{} version when we check it out), but we need to
        // de-dupe it
        let references = references.into_iter().unique().collect();
        Ok(references)
    }

    /// Filter references by looking at the date of their head commit and only including repos
    /// that have been updated in the last 30 days
    /// To do this we need a cloned repository so that we can run
    /// `git log <some format string that includes that date of the commit> <branch_or_tag_name>`
    /// in the cloned repo for each branch or tag
    fn references_that_need_scanning(
        references: Vec<RemoteReference>,
        integration: &Integration,
    ) -> Result<Vec<RemoteReference>, Report<RemoteProviderError>> {
        let tmpdir = tempdir()
            .into_report()
            .change_context(RemoteProviderError::RunCommand)
            .describe("creating temp directory in references_that_need_scanning")?;

        let repo = Repository {
            directory: PathBuf::from(tmpdir.path()),
            integration: integration.clone(),
        };
        repo.blobless_clone(None)
            .change_context(RemoteProviderError::RunCommand)
            .describe("cloning into temp directory in references_that_need_scanning")?;

        let initial_len = references.len();
        let filtered_references: Vec<RemoteReference> = references
            .into_iter()
            .filter(|reference| {
                repo.reference_needs_scanning(reference, PathBuf::from(tmpdir.path()))
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
        &self,
        reference: &RemoteReference,
        cloned_repo_dir: PathBuf,
    ) -> Result<bool, Report<RemoteProviderError>> {
        let mut reference_string = reference.reference.as_ref().to_string();
        if reference.ref_type == ReferenceType::Branch {
            reference_string = format!("origin/{reference_string}");
        }
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

        let output = self.run_git(args, Some(cloned_repo_dir))?;
        let date_strings = String::from_utf8_lossy(&output.stdout);
        println!(
            "author and committer date for {}: {}",
            reference.reference.as_ref(),
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

    /// The transport for this repository's integration
    fn transport(&self) -> git::transport::Transport {
        let remote::Protocol::Git(transport) = self.integration.protocol().clone();
        transport
    }

    /// Do a blobless clone of the repository, checking out the Reference if it exists
    fn blobless_clone(
        &self,
        reference: Option<&Reference>,
    ) -> Result<PathBuf, Report<RemoteProviderError>> {
        let directory = self.directory.to_string_lossy().to_string();
        let mut args = vec![String::from("clone"), String::from("--filter=blob:none")];
        if let Some(reference) = reference {
            args.append(&mut vec![
                String::from("--branch"),
                reference.as_ref().to_string(),
            ]);
        }
        args.append(&mut vec![
            self.transport().endpoint().as_ref().to_string(),
            directory.clone(),
        ]);
        self.run_git(args, None)?;
        Ok(PathBuf::from(directory))
    }

    fn default_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        let auth = self.transport().auth();
        // Credential helpers can override the header provided by http.extraHeader, so we need to get rid of them by setting `credential-helper` to ""
        // We only want to do this for the case where we are providing the http.extraHeader, so that we can use the credential helper by default
        let mut credential_helper_args =
            vec![String::from("-c"), String::from("credential.helper=''")];
        match auth {
            // git -c credential-helper="" -c http.extraHeader="AUTHORIZATION: Basic ${B64_GITHUB_TOKEN}" clone https://github.com/spatten/fanopticon
            git::transport::Auth::Http(Some(http::Auth::Basic { username, password })) => {
                let header = format!("{}:{}", username, password.as_ref().expose_secret());
                let base64_header = general_purpose::STANDARD.encode(header);
                let full_header =
                    format!("http.extraHeader=AUTHORIZATION: Basic {}", base64_header);
                let mut header_args = vec![String::from("-c"), full_header];
                args.append(&mut credential_helper_args);
                args.append(&mut header_args);
            }
            git::transport::Auth::Http(Some(http::Auth::Header(header))) => {
                let full_header = format!("http.extraHeader={}", header.as_ref().expose_secret());
                let mut header_args = vec![String::from("-c"), full_header];
                args.append(&mut credential_helper_args);
                args.append(&mut header_args);
            }
            _ => {}
        }
        args
    }

    fn env_vars(
        &self,
        ssh_key_file: &mut NamedTempFile<File>,
    ) -> Result<HashMap<String, String>, Report<RemoteProviderError>> {
        let mut env = HashMap::new();
        // Turn off terminal prompts if auth fails.
        env.insert(String::from("GIT_TERMINAL_PROMPT"), String::from("0"));

        let auth = self.transport().auth();
        match auth {
            git::transport::Auth::Ssh(ssh::Auth::KeyFile(path)) => {
                env.insert(
                    String::from("GIT_SSH_COMMAND"),
                    Self::git_ssh_command(path.display().to_string()),
                );
            }
            git::transport::Auth::Ssh(ssh::Auth::KeyValue(key)) => {
                // Write the contents of the SSH key to a file so that we can point to it in
                // GIT_SSH_COMMAND
                ssh_key_file
                    .write_all(key.as_ref().expose_secret().as_bytes())
                    .into_report()
                    .describe("writing ssh key to file")
                    .change_context(RemoteProviderError::RunCommand)?;

                env.insert(
                    String::from("GIT_SSH_COMMAND"),
                    Self::git_ssh_command(ssh_key_file.path().display().to_string()),
                );
            }
            _ => {}
        }
        Ok(env)
    }

    // git_ssh_command is passed into the GIT_SSH_COMMAND env variable. This makes git use this command
    // when it tries to make an SSH connection.
    // "-o IdentitiesOnly=yes" means "only use the identity file pointed to by the -i arg"
    // "-o StrictHostKeyChecking=no" avoids errors when the host is not in ssh's knownHosts file
    // "-F /dev/null" means "start with an empty ssh config"
    fn git_ssh_command(path: String) -> String {
        format!(
            "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -F /dev/null",
            path,
        )
    }

    fn run_git<I, S>(
        &self,
        args: I,
        cwd: Option<PathBuf>,
    ) -> Result<Output, Report<RemoteProviderError>>
    where
        I: IntoIterator<Item = S>,
        std::string::String: From<S>,
    {
        let mut full_args = self.default_args();
        let args_as_vec: Vec<String> = args.into_iter().map(String::from).collect();
        full_args.append(&mut args_as_vec.clone());

        let mut ssh_key_file = NamedTempFile::new()
            .into_report()
            .change_context(RemoteProviderError::RunCommand)
            .describe("creating temp file")?;
        let env = self.env_vars(&mut ssh_key_file)?;

        let mut command = Command::new("git");
        command.args(full_args).envs(env);
        println!("running git {:?} in directory {:?}", args_as_vec, cwd);
        if let Some(directory) = cwd {
            command.current_dir(directory);
        }
        let output = command
            .output()
            .into_report()
            .change_context(RemoteProviderError::RunCommand)
            .describe_lazy(|| format!("running git command {:?}", args_as_vec))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RemoteProviderError::RunCommand)
                .into_report()
                .describe_lazy(|| {
                    format!(
                        "running git command {:?}, status was: {}, stderr: {}",
                        args_as_vec, output.status, stderr,
                    )
                });
        }
        Ok(output)
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
    fn parse_ls_remote(
        output: String,
    ) -> Result<Vec<RemoteReference>, Report<RemoteProviderError>> {
        let results: Vec<RemoteReference> = output
            .split('\n')
            .map(Self::line_to_git_ref)
            .filter(|r| r.is_some())
            .flatten()
            .collect();
        Ok(results)
    }

    fn line_to_git_ref(line: &str) -> Option<RemoteReference> {
        let mut parsed = line.split_whitespace();
        let commit = parsed.next()?;
        let commit = String::from(commit);
        let reference = parsed.next()?;

        if let Some(tag) = reference.strip_prefix("refs/tags/") {
            if tag.ends_with("^{}") {
                return None;
            }
            return Some(RemoteReference {
                ref_type: ReferenceType::Tag,
                commit: Commit(commit),
                reference: Reference(String::from(tag)),
            });
        }

        if let Some(branch) = reference.strip_prefix("refs/heads/") {
            return Some(RemoteReference {
                ref_type: ReferenceType::Branch,
                commit: Commit(commit),
                reference: Reference(String::from(branch)),
            });
        }

        None
    }
}
