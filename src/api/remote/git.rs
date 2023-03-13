pub mod repository;
pub mod transport;
use derive_new::new;

/// A git reference's type (branch or tag)
#[derive(Debug, Clone, Hash, Eq, PartialEq, new)]
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

impl Reference {
    fn name(&self) -> &String {
        match self {
            Self::Branch { name, .. } => name,
            Self::Tag { name, .. } => name,
        }
    }

    fn table_row(&self) -> String {
        match self {
            Self::Branch { name, head } => format!("branch\t\t{}\t\t{}", name, head),
            Self::Tag { name, commit } => format!("tag\t\t{}\t\t{}", name, commit),
        }
    }
}
