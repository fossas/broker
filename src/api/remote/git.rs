pub mod repository;
pub mod transport;
use derive_new::new;
use tabled::Tabled;

/// A git reference's type (branch or tag)
#[derive(Debug, Clone, Hash, Eq, PartialEq, new, Tabled)]
pub enum Reference {
    /// A branch
    #[tabled(inline("Branch::"))]
    Branch {
        /// The name of the branch
        name: String,

        /// The head commit of the branch
        head: String,
    },

    /// A tag
    #[tabled(inline("Tag::"))]
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
}
