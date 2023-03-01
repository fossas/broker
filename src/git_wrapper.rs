//! Wrapper for Git
use std::process::{Command, Output};

use error_stack::{IntoReport, Report, ResultExt};

use crate::{
    api::http,
    api::remote::git,
    api::remote::Remote,
    ext::{error_stack::DescribeContext, secrecy::ComparableSecretString},
};

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
// pub struct Repository {
//     /// directory is the location on disk where the repository resides or will reside
//     pub directory: String,
//     /// safe_url is the URL of the repository with no authentication info
//     pub safe_url: String,
//     /// auth is the authentication info for the repository
//     pub auth: GitAuth,
//     /// checkout_type is the state of the repository
//     pub checkout_type: CheckoutType,
// }

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

    fn remote_with_auth(&self) -> Result<Remote, Report<Error>> {
        let safe_url = self.transport.endpoint().clone();
        if let git::Auth::Http(Some(http::Auth::Basic { username, password })) =
            self.transport.auth()
        {
            Self::add_auth_to_remote(safe_url, &password.clone(), &username.clone())
        } else {
            Ok(safe_url)
        }
    }

    fn add_auth_to_remote(
        url: Remote,
        password: &ComparableSecretString,
        username: &String,
    ) -> Result<Remote, Report<Error>> {
        let parsed_url = url.parse();
        match parsed_url {
            Ok(mut url) => {
                // let res = url.set_password(password.map(|p| p.as_str()));
                let res = url.set_password(Some(&format!("{:?}", password)));

                if let Err(_) = res {
                    return Err(Error::ParseUrl).into_report();
                }
                let res = url.set_username(username.as_str());
                if let Err(_) = res {
                    return Err(Error::ParseUrl).into_report();
                }

                Remote::try_from(url.to_string()).change_context(Error::ParseUrl)
            }
            Err(_) => Err(Error::ParseUrl).into_report(),
        }
    }

    /// Do a blobless clone of the repository
    pub fn git_clone(self) -> Result<Self, Report<Error>> {
        let remote_with_auth = self.remote_with_auth()?;
        Self::run_git(&[
            String::from("clone"),
            String::from("--filter=blob:none"),
            format!("{}", remote_with_auth),
            self.directory.clone(),
        ])
        .and_then(|_| {
            let repo = Repository {
                checkout_type: CheckoutType::Blobless,
                ..self
            };
            Ok(repo)
        })
    }

    fn run_git(args: &[String]) -> Result<Output, Report<Error>> {
        let output = Command::new("git")
            .args(args)
            .output()
            .into_report()
            .change_context(Error::RunCommand)
            .describe_lazy(|| format!("running git command {:?}", args))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::RunCommand).into_report().describe_lazy(|| {
                format!(
                    "running git command {:?}, status was: {}, stderr: {}",
                    args, output.status, stderr
                )
            });
        }
        Ok(output)
    }
}
