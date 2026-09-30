mod align;
mod convert;
mod edit;
mod primer;
mod project;
mod view;

// Test-facing re-exports: mcp/tests.rs globs this module's scope through
// `use super::*`; several items are unused outside cfg(test).
#[allow(unused_imports)]
pub(crate) use align::*;
#[allow(unused_imports)]
pub(crate) use convert::*;
#[allow(unused_imports)]
pub(crate) use edit::*;
#[allow(unused_imports)]
pub(crate) use primer::*;
#[allow(unused_imports)]
pub(crate) use project::*;
#[allow(unused_imports)]
pub(crate) use view::*;
