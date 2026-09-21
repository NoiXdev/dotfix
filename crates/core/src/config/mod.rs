mod desired;
pub mod edit;
mod machine;
mod repo;
mod set;

pub use desired::{Desired, Fragment, ResolvedFile};
pub use machine::{MachineConfig, ProviderKind};
pub use repo::{Repo, RepoConfig, SCHEMA_VERSION};
pub use set::{FileMode, ManagedFile, Packages, Requirement, SetConfig};
