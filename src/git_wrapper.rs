//! Wrapper for Git

use std::process::Command;

use error_stack::{IntoReport, Report};
use url::Url;

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
pub enum CheckoutType {
    /// not checked out yet
    None,
    /// initialized with git init; git remote add origin <url>
    Inited,
    /// Blobless clone
    Blobless,
}

/// Name and password for basis auth
#[derive(Debug, Clone)]
pub struct NameAndPassword {
    name: String,
    password: String,
}

/// username and path to the private key for SSH auth
#[derive(Debug, Clone)]
pub struct NameAndPath {
    /// The username for SSH auth
    name: String,
    /// The path to the private key for SSH auth
    path: String,
}

/// Auth type for a git repo
#[derive(Debug, Clone)]
pub enum GitAuth {
    /// No authentication
    NoAuth,
    /// Authentication via a token
    TokenAuth(String),
    /// Basic auth via name and password
    BasicAuth(NameAndPassword),
    /// Auth via an SSH key stored on disk
    SSHAuth(NameAndPath),
}

/// A git repository
pub struct Repository {
    /// directory is the location on disk where the repository resides or will reside
    pub directory: String,
    /// safe_url is the URL of the repository with no authentication info
    pub safe_url: String,
    /// auth is the authentication info for the repository
    pub auth: GitAuth,
    /// checkout_type is the state of the repository
    pub checkout_type: CheckoutType,
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

    fn remote_with_auth(&self) -> Result<String, Report<Error>> {
        match &self.auth {
            GitAuth::NoAuth => Ok(self.safe_url.clone()),
            GitAuth::TokenAuth(token) => Self::add_auth_to_remote(
                &self.safe_url.clone(),
                Some(token),
                String::from("auth-x"),
            ),
            GitAuth::BasicAuth(auth_info) => Self::add_auth_to_remote(
                &self.safe_url.clone(),
                Some(&auth_info.password),
                auth_info.name.clone(),
            ),
            GitAuth::SSHAuth(auth_info) => {
                Self::add_auth_to_remote(&self.safe_url.clone(), None, auth_info.name.clone())
            }
        }
    }

    fn add_auth_to_remote(
        url: &String,
        password: Option<&String>,
        username: String,
    ) -> Result<String, Report<Error>> {
        let parsed_url = Url::parse(&url[..]);
        match parsed_url {
            Ok(mut url) => {
                let res = url.set_password(password.map(|p| p.as_str()));
                if let Err(_) = res {
                    return Err(Error::ParseUrl).into_report();
                }
                let res = url.set_username(username.as_str());
                if let Err(_) = res {
                    return Err(Error::ParseUrl).into_report();
                }

                Ok(url.to_string())
            }
            Err(_) => Err(Error::ParseUrl).into_report(),
        }
    }

    /// Do a blobless clone of the repository
    pub fn clone(&self) -> Result<Self, Report<Error>> {
        let remote = self.remote_with_auth();
        match remote {
            Err(rem) => Err(rem),
            Ok(rem) => {
                let res = Self::run_git(&[
                    String::from("clone"),
                    String::from("--filter=blob:none"),
                    rem,
                    self.directory.clone(),
                ]);
                match res {
                    Ok(_) => Ok(Repository {
                        checkout_type: CheckoutType::Blobless,
                        directory: self.directory.clone(),
                        safe_url: self.safe_url.clone(),
                        auth: self.auth.clone(),
                    }),
                    Err(err) => Err(err),
                }
            }
        }
    }

    fn run_git(args: &[String]) -> Result<(), Report<Error>> {
        let output = Command::new("git")
            .args(args)
            .output()
            .expect("error running git");
        if output.status.success() {
            return Ok(());
        }
        return Err(Error::RunCommand).into_report();
    }
}
