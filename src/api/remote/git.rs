pub mod repository;
pub mod transport;
use std::fmt::Display;

use derive_new::new;
use serde::{Deserialize, Serialize};

/// A git reference's type (branch or tag)
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize, new)]
pub enum Reference {
    /// A branch
    Branch {
        /// The name of the branch
        name: String,

        /// The head commit of the branch
        head: String,
    },

    /// A tag
    Tag {
        /// The name of the tag
        name: String,
        /// The commit that the tag points at
        commit: String,
    },
}

impl Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reference::Branch { name, head } => {
                write!(f, "branch::{name}@{head}")
            }
            Reference::Tag { name, commit } => {
                write!(f, "tag::{name}@{commit}")
            }
        }
    }
}

impl Reference {
    /// Retrieves name of reference
    pub fn name(&self) -> &String {
        match self {
            Self::Branch { name, .. } => name,
            Self::Tag { name, .. } => name,
        }
    }

    /// Generate a canonical state for the reference.
    pub fn as_state(&self) -> &[u8] {
        match self {
            Reference::Branch { head, .. } => head.as_bytes(),
            Reference::Tag { commit, .. } => commit.as_bytes(),
        }
    }

    /// Generate a representation for the reference suitable for use when
    /// creating database coordinates.
    pub fn for_coordinate(&self) -> String {
        match self {
            Reference::Branch { name, head } => format!("branch:{name}@{head}"),
            Reference::Tag { name, commit } => format!("tag:{name}@{commit}"),
        }
    }
}
