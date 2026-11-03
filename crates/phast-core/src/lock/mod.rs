//! Lock file types and content hashing.
//!
//! A resolution input reaches the lock one of two ways: recorded as data and readable back
//! out of it, or folded into the content hash and invisible. `config.platform` and
//! `config.minimum-stability` take the second route, so looking for them among the stored
//! fields will not turn them up. Changing either dirties the lock without altering anything
//! you can see in the file.

pub mod hash;
pub mod phast;
pub mod sort;
