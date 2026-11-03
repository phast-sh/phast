//! Running the dependency solver against the workspace manifest.
//!
//! [`Resolver`] owns the registry connection details and the progress sink; the
//! workspace is borrowed per call. It returns both the resolution and the
//! provider whose warmed metadata cache the Locker reuses for enrichment, so the
//! lock-writing phase makes no extra network requests.

use super::{explain, requirements};
use crate::output::Printer;
use crate::platform;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use phast_core::PackageName;
use phast_core::git;
use phast_core::lock::phast::PhastLock;
use phast_registry::http::client::HttpClient;
use phast_registry::provider::ChainedProvider;
use phast_registry::router::RegistryOptions;
use phast_resolver::provider::Provider;
use phast_resolver::resolution::Resolution;
use phast_resolver::{Requirement, ResolveOptions, resolve};
use phast_version::Version;
use phast_version::parser::parse_version;
use phast_workspace::Workspace;

use anyhow::{Result, bail};

/// What a resolution asks for beyond the plain case: what to hold steady, and what to
/// require that the manifest does not.
#[derive(Default)]
pub struct ResolveRequest {
    /// Packages held at an exact version, so a selective update cannot move them.
    pub pins: HashMap<PackageName, Version>,
    /// Requirements the manifest does not declare yet, layered on top of the ones it does.
    pub proposed: Vec<Requirement>,
}

/// A resolution and the provider that produced it.
///
/// The two travel together because the lock is enriched from the provider's metadata
/// cache, warmed by the very solve that produced this resolution. Pairing them here is
/// what stops a caller handing the locker a cold provider, which would answer the same
/// questions over the network.
pub struct Resolved {
    /// What the solver selected.
    pub resolution: Resolution,
    /// The provider that answered the solve, with its metadata cache populated.
    pub provider: ChainedProvider,
}

/// Runs the dependency solver and produces a [`Resolved`].
pub struct Resolver {
    /// Registry URL override from `--registry-url`. `None` means use manifest repositories.
    registry_url: Option<String>,
    /// HTTP client with credentials baked in, shared by resolution and download phases.
    http_client: HttpClient,
    /// Cache root directory, respecting --cache-dir / PHAST_CACHE_DIR overrides.
    cache_dir: PathBuf,
    /// Progress and status output sink.
    printer: Printer,
}

impl Resolver {
    pub fn new(
        registry_url: Option<String>,
        http_client: HttpClient,
        cache_dir: PathBuf,
        printer: Printer,
    ) -> Self {
        Self {
            registry_url,
            http_client,
            cache_dir,
            printer,
        }
    }

    /// Resolve the workspace manifest and return the resolution with the provider.
    ///
    /// `lock` does three jobs. Its versions become the preference map, so the solver stays
    /// near what is already installed. Its `packages_dev` entries name what was dev-only last
    /// time, which is how a `--no-dev` solve knows which pins to drop rather than inject as
    /// root-level requirements. And its package list decides the prefetch: given a lock every
    /// locked name is warmed at once, and without one only the direct requirements are, so a
    /// first resolve discovers the rest of the graph as it walks it.
    #[rustfmt::skip]
    pub async fn resolve(
        &self,
        lock: Option<PhastLock>,
        request: ResolveRequest,
        workspace: &Workspace,
    ) -> Result<Resolved> {
        // Dev requirements always take part. The lock answers the manifest as a whole, and
        // `--no-dev` only narrows what an install places, so a lock written by a production
        // build still installs the dev packages on the next plain `install`.
        let mut requirements = requirements::collect(workspace.manifest(), true);
        requirements.extend(request.proposed);

        let platform_map = platform::build_platform_map(workspace.manifest())?;

        let spinner = self.printer.spinner("Resolving dependencies...");
        let start = Instant::now();

        let provider = match self.registry_url.as_deref() {
            Some(url) => {
                ChainedProvider::new(
                    self.http_client.clone(),
                    self.cache_dir.clone()
                ).with_url(
                    workspace.root(),
                    url,
                    RegistryOptions::from_manifest(workspace.manifest()),
                ).await?
            },
            None => {
                ChainedProvider::new(
                    self.http_client.clone(),
                    self.cache_dir.clone()
                ).with_manifest(
                    workspace.root(),
                    workspace.manifest(),
                    RegistryOptions::from_manifest(workspace.manifest()),
                    |w| self.printer.warning(w),
                ).await?
            },
        };

        let prefetch_names: Vec<PackageName> = lock
        .as_ref()
        .map(|l| {
            l.packages
                .iter()
                .chain(l.packages_dev.iter())
                .map(|p| PackageName::new(&p.name))
                .collect()
        })
        .unwrap_or_else(|| {
            requirements
                .iter()
                .map(|r| r.name.clone())
                .collect()
        });
        provider.prefetch(&prefetch_names).await;

        // The root's own version, so its require/provide/replace self.version
        // resolves to it. Precedence mirrors a path package: the declared version,
        // then a `dev-<branch>` guess from the checkout (a version-less monorepo root).
        // Absent both, the resolver falls back to the synthetic root version.
        let root_version = workspace
            .manifest()
            .version
            .as_deref()
            .and_then(|v| parse_version(v).ok())
            .or_else(|| git::current_branch(workspace.root()).map(Version::Branch));

        let preferences: HashMap<PackageName, Version> = lock
            .iter()
            .flat_map(|l| l.packages.iter().chain(l.packages_dev.iter()))
            .filter_map(|p| Some((PackageName::new(&p.name), parse_version(&p.version).ok()?)))
            .collect();

        let dev_package_names: HashSet<PackageName> = lock
            .iter()
            .flat_map(|l| l.packages_dev.iter())
            .map(|p| PackageName::new(&p.name))
            .collect();

        let options = ResolveOptions {
            root_version,
            preferences,
            locked_pins: request.pins,
            provide: workspace.manifest().provide.clone(),
            replace: workspace.manifest().replace.clone(),
            conflict: workspace.manifest().conflict.clone(),
            dev_package_names,
        };

        let resolution = match resolve(requirements, provider.clone(), platform_map, options).await {
            Ok(resolution) => resolution,
            Err(error) => {
                let err = error.to_string();
                bail!("{err}{}", explain::rejections(&provider, &err));
            },
        };

        spinner.finish_and_clear();

        let total = resolution.packages.len();
        self.printer.phase("Resolved", total, start.elapsed().as_secs_f64());

        Ok(Resolved { resolution, provider })
    }
}
