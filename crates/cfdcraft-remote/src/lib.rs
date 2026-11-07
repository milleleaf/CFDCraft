//! The compute host as the drain sees it: a narrow `Host` trait, an in-memory
//! `FakeBox` for loop-level tests and, with the command builders, the real
//! implementation over ssh and the forwarded container socket.

pub mod commands;
pub mod fake;
pub mod host;
pub mod ssh_host;

pub use cfdcraft_executor::{Holder, ReleaseOutcome};
pub use host::{
    ContainerState, Host, HostError, RemoteFile, TimeDir, is_owned, is_time_name, valid_rel,
    valid_run_id,
};
pub use ssh_host::SshHost;
