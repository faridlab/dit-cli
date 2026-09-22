//! The only crate that sends a request whose destination came from a file
//! (invariant I11, §20.5).
//!
//! It is handed a plan — a chain of steps, each already resolved to a method
//! and a path, plus the base URL, the environment's values and the hosts this
//! machine allows — and it returns what happened. It reads no files, touches
//! no index and knows nothing about git or the workspace: there is no way to
//! reach the network *through* it from a read path, because it has no way to
//! be handed a workspace in the first place.
//!
//! Nothing here runs unless someone asked, in that moment. The caller is
//! `dit morse run`, `dit morse sync`, or the Run control — never reindex, the
//! watcher, `doctor`, or a server read handler.

pub mod jsonpath;
pub mod local;
pub mod run;
pub mod template;

pub use local::{LocalConfig, LocalEnv, LocalError, ALLOW_HOSTS_VAR, VARS_VAR};
pub use run::{run, PlannedStep, Policy, RunOutcome, RunPlan, StepOutcome};
