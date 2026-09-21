mod brew;
mod exec;
#[cfg(any(test, feature = "fakes"))]
pub mod fake;
mod fsys;
mod git;

pub use brew::{Brew, RealBrew};
pub use exec::{Exec, RealExec};
pub use fsys::{Fsys, RealFsys};
pub use git::{Commit, Git, RealGit};
