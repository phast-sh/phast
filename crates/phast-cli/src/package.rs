//! Package names as they arrive from the command line.
//!
//! Every command taking a `vendor/package` argument parses it here, so a malformed name
//! is reported as one wherever it appears, rather than as whatever the command would
//! have failed to find afterward.
//!
//! The check is shallow, though: a name is accepted if it contains a slash at all, so
//! `notapackage` is refused here while `a/b/c` and `a/` get through and surface later as a
//! package nothing has heard of.

use crate::hints;

use phast_core::package::PackageName;

use anyhow::{Result, bail};

/// A package name from a CLI argument, with the constraint that followed it.
pub struct PackageRequest {
    /// The `vendor/package` name, e.g. `psr/log`.
    pub name: String,
    /// The constraint after the colon, e.g. `^3.0`. `None` when not supplied.
    pub constraint: Option<String>,
}

impl PackageRequest {
    /// Parse `vendor/package`, `vendor/package:constraint`, or a platform package.
    ///
    /// The constraint is carried for whoever wants it and ignored by everyone else, so
    /// `psr/log:^3.0` names the same package to every command instead of being read as a
    /// name that happens to contain a colon.
    pub fn parse(arg: &str) -> Result<Self> {
        let (name, constraint) = match arg.split_once(':') {
            Some((name, constraint)) => (name, Some(constraint.to_owned())),
            None => (arg, None),
        };

        if !PackageName::is_valid(name) {
            bail!(hints::invalid_package_name(name));
        }

        Ok(Self {
            name: name.to_owned(),
            constraint,
        })
    }

    /// Parse every argument, failing on the first invalid name.
    pub fn parse_all<'a>(args: impl IntoIterator<Item = &'a String>) -> Result<Vec<Self>> {
        args.into_iter().map(|arg| Self::parse(arg)).collect()
    }

    /// The parsed names, dropping any constraints.
    ///
    /// Normalised on the way out, so a caller that only needs to identify packages is not
    /// left holding strings that may differ from the same name written another way.
    pub fn names<'a>(args: impl IntoIterator<Item = &'a String>) -> Result<Vec<PackageName>> {
        Ok(Self::parse_all(args)?
            .iter()
            .map(|request| PackageName::new(&request.name))
            .collect())
    }
}
