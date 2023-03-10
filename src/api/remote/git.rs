pub mod repository;
pub mod transport;
use derive_new::new;

/// A git reference's type (branch or tag)
#[derive(Debug, Clone, Hash, Eq, PartialEq, new)]
pub enum Reference {
    /// A branch
    Branch { name: String, head: String },

    /// A tag
    Tag { name: String, commit: String },
}

impl Reference {
    fn name(&self) -> &String {
        match self {
            Self::Branch { name, .. } => name,
            Self::Tag { name, .. } => name,
        }
    }
}
