#[allow(clippy::module_inception)]
mod tac;
pub use self::tac::*;
pub mod instructions;
mod optimizer;
pub use optimizer::*;
