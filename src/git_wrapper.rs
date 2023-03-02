//! Wrapper for Git
use base64::{engine::general_purpose, Engine as _};
use error_stack::{IntoReport, Report, ResultExt};
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::process::{Command, Output};
use tempfile::NamedTempFile;

use crate::{api::http, api::remote::git, api::ssh, ext::error_stack::DescribeContext};

/// Errors that are encountered while shelling out to git.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// We encountered an error while shelling out to git
    #[error("running git command")]
    RunCommand,
    /// We encountered an error while parsing the repository's URL
    #[error("parsing url")]
    ParseUrl,
}

/// The checkout type of the repository
#[derive(Debug)]
pub enum CheckoutType {
    /// not checked out yet
    None,
    /// initialized with git init; git remote add origin <url>
    Inited,
    /// Blobless clone
    Blobless,
}

/// A git repository
#[derive(Debug)]
pub struct Repository {
    /// directory is the location on disk where the repository resides or will reside
    pub directory: String,
    /// checkout_type is the state of the repository
    pub checkout_type: CheckoutType,
    /// transport contains the info that Broker uses to communicate with the git host
    pub transport: git::Transport,
}

impl Repository {
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
    pub fn git_clone(self) -> Result<Self, Report<Error>> {
        let args = vec![
            String::from("clone"),
            String::from("--filter=blob:none"),
            self.transport.endpoint().as_ref().to_string(),
            self.directory.clone(),
        ];
        self.run_git(args).and_then(|_| {
            let repo = Repository {
                checkout_type: CheckoutType::Blobless,
                ..self
            };
            Ok(repo)
        })
    }

    fn default_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        let auth = self.transport.auth();
        // Credential helpers can override the header provided by http.extraHeader, so we need to get rid of them by setting `credential-helper` to ""
        // We only want to do this for the case where we are providing the http.extraHeader, so that we can use the credential helper by default
        let mut credential_helper_args =
            vec![String::from("-c"), String::from("credential.helper=''")];
        match auth {
            // git -c credential-helper="" -c http.extraHeader="AUTHORIZATION: Basic ${B64_GITHUB_TOKEN}" clone https://github.com/spatten/fanopticon
            git::Auth::Http(Some(http::Auth::Basic { username, password })) => {
                let header = format!("{}:{}", username, password.as_ref().expose_secret());
                let base64_header = general_purpose::STANDARD.encode(header);
                let full_header =
                    format!("http.extraHeader=AUTHORIZATION: Basic {}", base64_header);
                let mut header_args = vec![String::from("-c"), String::from(full_header)];
                args.append(&mut credential_helper_args);
                args.append(&mut header_args);
            }
            git::Auth::Http(Some(http::Auth::Header(header))) => {
                let full_header = format!("http.extraHeader={}", header.as_ref().expose_secret());
                let mut header_args = vec![String::from("-c"), String::from(full_header)];
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
    ) -> Result<HashMap<String, String>, Report<Error>> {
        let mut env = HashMap::new();
        env.insert(String::from("GIT_TERMINAL_PROMPT"), String::from("0"));

        let auth = self.transport.auth();
        match auth {
            git::Auth::Ssh(Some(ssh::Auth::KeyFile(path))) => {
                let git_ssh_command = format!(
                    "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -F /dev/null",
                    path.display(),
                );
                env.insert(String::from("GIT_SSH_COMMAND"), git_ssh_command);
            }
            git::Auth::Ssh(Some(ssh::Auth::KeyValue(key))) => {
                // Write the contents of the SSH key to a file so that we can point to it in
                // GIT_SSH_COMMAND
                ssh_key_file
                    .write_all(key.as_ref().expose_secret().as_bytes())
                    .into_report()
                    .describe("writing ssh key to file")
                    .change_context(Error::RunCommand)?;

                let git_ssh_command = format!(
                    "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -F /dev/null",
                    ssh_key_file.path().display(),
                );
                env.insert(String::from("GIT_SSH_COMMAND"), git_ssh_command);
            }
            _ => {}
        }
        Ok(env)
    }

    fn run_git(&self, args: Vec<String>) -> Result<Output, Report<Error>> {
        let mut full_args = self.default_args();
        full_args.append(&mut args.clone());

        let mut ssh_key_file = NamedTempFile::new()
            .into_report()
            .change_context(Error::RunCommand)
            .describe("creating temp file")?;
        let env = self.env_vars(&mut ssh_key_file)?;

        let output = Command::new("git")
            .args(full_args)
            .envs(env)
            .output()
            .into_report()
            .change_context(Error::RunCommand)
            .describe_lazy(|| format!("running git command {:?}", args))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::RunCommand).into_report().describe_lazy(|| {
                format!(
                    "running git command {:?}, status was: {}, stderr: {}",
                    args, output.status, stderr,
                )
            });
        }
        Ok(output)
    }
}
