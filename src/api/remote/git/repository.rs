//! Wrapper for Git
use base64::{engine::general_purpose, Engine as _};
use error_stack::{IntoReport, Report, ResultExt};
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::NamedTempFile;

use crate::api::remote::{
    Commit, Reference, ReferenceType, RemoteProvider, RemoteProviderError, RemoteReference,
};
use crate::{api::http, api::remote::git, api::ssh, ext::error_stack::DescribeContext};

/// A git repository
#[derive(Debug)]
pub struct Repository {
    /// directory is the location on disk where the repository resides or will reside
    pub directory: PathBuf,
    /// transport contains the info that Broker uses to communicate with the git host
    pub transport: git::transport::Transport,
}

impl RemoteProvider for Repository {
    // mkdir <directory>
    // git init
    // git remote add origin <url>
    // git ls-remote
    // fn init(repo: Repository) -> Option<Repository> {
    //     if repo.checkout_type != None {
    //         return None;
    //     }

    //     let repo = init_repo(repo);
    //     let repo = set_remote(repo);
    //     check_remote(repo)
    // }

    /// Do a blobless clone of the repository
    fn clone(self) -> Result<PathBuf, Report<RemoteProviderError>> {
        let directory = self.directory.to_string_lossy().to_string();
        let args = vec![
            String::from("clone"),
            String::from("--filter=blob:none"),
            self.transport.endpoint().as_ref().to_string(),
            directory.clone(),
        ];
        self.run_git(args)?;
        Ok(PathBuf::from(directory))
    }

    fn fetch(self) -> Result<(), Report<RemoteProviderError>> {
        let args = vec![String::from("fetch"), String::from("origin")];

        self.run_git(args)?;
        Ok(())
    }

    fn get_references(self) -> Result<Vec<RemoteReference>, Report<RemoteProviderError>> {
        let args = vec![String::from("ls-remote"), String::from("--quiet")];

        let output = self.run_git(args)?;
        let output = String::from_utf8(output.stdout)
            .into_report()
            .describe("reading output of 'git ls-remote --quiet'")
            .change_context(RemoteProviderError::RunCommand)?;
        Self::parse_ls_remote(output)
    }
}

impl Repository {
    fn default_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        let auth = self.transport.auth();
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
        env.insert(String::from("GIT_TERMINAL_PROMPT"), String::from("0"));

        let auth = self.transport.auth();
        match auth {
            git::transport::Auth::Ssh(Some(ssh::Auth::KeyFile(path))) => {
                env.insert(
                    String::from("GIT_SSH_COMMAND"),
                    Self::git_ssh_command(path.display().to_string()),
                );
            }
            git::transport::Auth::Ssh(Some(ssh::Auth::KeyValue(key))) => {
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

    fn run_git(&self, args: Vec<String>) -> Result<Output, Report<RemoteProviderError>> {
        let mut full_args = self.default_args();
        full_args.append(&mut args.clone());

        let mut ssh_key_file = NamedTempFile::new()
            .into_report()
            .change_context(RemoteProviderError::RunCommand)
            .describe("creating temp file")?;
        let env = self.env_vars(&mut ssh_key_file)?;

        let output = Command::new("git")
            .args(full_args)
            .envs(env)
            .output()
            .into_report()
            .change_context(RemoteProviderError::RunCommand)
            .describe_lazy(|| format!("running git command {:?}", args))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RemoteProviderError::RunCommand)
                .into_report()
                .describe_lazy(|| {
                    format!(
                        "running git command {:?}, status was: {}, stderr: {}",
                        args, output.status, stderr,
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
    fn parse_ls_remote(
        output: String,
    ) -> Result<Vec<RemoteReference>, Report<RemoteProviderError>> {
        let results: Vec<RemoteReference> = output
            .split("\n")
            .map(|line| Self::line_to_git_ref(line))
            .filter(|r| r.is_some())
            .map(|r| r.unwrap())
            .collect();
        Ok(results)
    }

    fn line_to_git_ref(line: &str) -> Option<RemoteReference> {
        let mut parsed = line.split_whitespace();
        let commit = parsed.next()?;
        let commit = String::from(commit);
        let reference = parsed.next()?;

        if let Some(tag) = reference.strip_prefix("refs/tags/") {
            return Some(RemoteReference {
                ref_type: ReferenceType::Tag,
                commit: Commit(commit),
                reference: Reference(String::from(tag.clone())),
            });
        }

        if let Some(branch) = reference.strip_prefix("refs/heads/") {
            return Some(RemoteReference {
                ref_type: ReferenceType::Branch,
                commit: Commit(commit),
                reference: Reference(String::from(branch.clone())),
            });
        }

        None
    }
}
