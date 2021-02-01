//! Check Maven Central for the latest version(s) of some maven coordinates.
//!
//! # Building
//!
//! ## Prerequisites
//!
//! This tool is build with Rust so you need to have a rust toolchain and cargo installed.
//! If you don't, please visit [https://rustup.rs/](https://rustup.rs/) and follow their instructions.
//!
//! ## Building
//!
//! The preferred way is to run:
//!
//! ```
//! make install
//! ```
//! If you do not have a fairly recent make (on macOS, homebrew can install a newer version),
//! or don't want to use make, you can also run `cargo install --path .`.
//!
//! # Usage
//!
//! Run `latest-maven-version --help` for an overview of all available options.
//!
//! The main usage is by providing maven coordinates in the form of `groupId:artifact`, followed by multiple `:version` qualifiers.
//! These version qualifier are [Semantic Version Ranges](https://www.npmjs.com/package/semver#advanced-range-syntax).
//! For each of the provided versions, the latest available version on maven central is printed.
//!
//! ### Default version
//!
//! The version ranges can be left out, in which case the latest overall version is printed.
//!
//! ### Multiple Version ranges
//!
//! You can also enter multiple coordinates, each with their own versions to check against.
//! The result is printed after all versions were checked successfully.
//!
//! ### Pre Release Versions
//!
//! Pre-releases can be included with the `--include-pre-releases` flag (or `-i` for short).
//!
//! ### Version overrides
//!
//! The versions are matched in order and a single version can only be matched by one qualifier.
//! Previous matches will – depending on the range – consume all versions that would have also been matched by later qualifiers.
//! Try to define the qualifiers in the order from most restrictive to least.
//!
//! # Examples
//!
//! Matching against minor-compatible releases.
//!
//!     $ latest-maven-version org.neo4j.gds:proc:~1.1:~1.3:1
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching ~1.1: 1.1.4
//!     Latest version matching ~1.3: 1.3.1
//!     Latest version matching ^1: 1.2.3
//!
//!
//! Matching against major compatible releases. Note that `1.3` does not produce any match, as it is already covered by `1.1`.
//!
//!     $ latest-maven-version org.neo4j.gds:proc:1.1:1.3:1
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching ^1.1: 1.3.1
//!     No version matching ^1.3
//!     Latest version matching ^1: 1.0.0
//!
//!
//! Inclusion of pre releases.
//!
//!     $ latest-maven-version org.neo4j.gds:proc:~1.1:~1.3:1 --include-pre-releases
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching ~1.1: 1.1.4
//!     Latest version matching ~1.3: 1.3.1
//!     Latest version matching ^1: 1.4.0-alpha02
//!
//!
//! Default version.
//!
//!     $ latest-maven-version org.neo4j.gds:proc
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching *: 1.3.1
//!
//!     $ latest-maven-version org.neo4j.gds:proc --include-pre-releases
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching *: 1.4.0-alpha02
//!
//!
//! Multiple checks.
//!
//!     $ latest-maven-version org.neo4j.gds:proc org.neo4j:neo4j
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching *: 1.3.1
//!     Latest version(s) for org.neo4j:neo4j:
//!     Latest version matching *: 4.1.1
//!
//!
use color_eyre::eyre::Result;
use console::{style, Term};
use resolvers::{Client, Resolver, UrlResolver};
use semver::{Version, VersionReq};
use std::sync::Arc;
use versions::Versions;

mod metadata;
mod opts;
mod resolvers;
mod versions;

#[tokio::main]
async fn main() -> Result<()> {
    if Term::stdout().features().is_attended() {
        color_eyre::config::HookBuilder::default()
            .display_env_section(false)
            .install()?
    }

    let mut opts = opts::Opts::new();
    let config = opts.config();

    let server = opts.resolver_server();
    let resolver = UrlResolver::new(server.url, server.auth)?;
    let client = resolvers::client();

    let checks = opts.into_version_checks();

    let results = run(resolver, client, config, checks).await?;

    for CheckResult {
        coordinates,
        versions,
    } in results
    {
        println!(
            "Latest version(s) for {}:{}:",
            style(coordinates.group_id).magenta(),
            style(coordinates.artifact).blue()
        );

        for (req, latest) in versions {
            if let Some(latest) = latest {
                println!(
                    "Latest version matching {}: {}",
                    style(req).cyan().bold(),
                    style(latest).green().bold()
                );
            } else {
                println!("No version matching {}", style(req).yellow().bold());
            }
        }
    }

    Ok(())
}

async fn run<R, C>(
    resolver: R,
    client: C,
    config: Config,
    checks: Vec<VersionCheck>,
) -> Result<Vec<CheckResult>>
where
    R: Resolver + Send + Sync + 'static,
    C: Client + Send + Sync + 'static,
{
    let resolver = Arc::new(resolver);
    let client = Arc::new(client);

    let tasks = checks
        .into_iter()
        .map(|check| {
            let resolver = Arc::clone(&resolver);
            let client = Arc::clone(&client);
            tokio::spawn(run_check(
                resolver,
                client,
                config.include_pre_releases,
                check,
            ))
        })
        .collect::<Vec<_>>();

    let mut results = Vec::with_capacity(tasks.len());
    for task in tasks {
        let result = task.await??;
        results.push(result);
    }
    Ok(results)
}

async fn run_check(
    resolver: Arc<impl Resolver>,
    client: Arc<impl Client>,
    include_pre_releases: bool,
    check: VersionCheck,
) -> Result<CheckResult> {
    let VersionCheck {
        coordinates,
        versions,
    } = check;

    let all_versions = resolver.resolve(&coordinates, &*client).await?;
    let versions = all_versions.latest_versions(include_pre_releases, versions);
    Ok(CheckResult {
        coordinates,
        versions,
    })
}

#[derive(Debug)]
struct Server {
    url: String,
    auth: Option<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
struct Config {
    include_pre_releases: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Coordinates {
    group_id: String,
    artifact: String,
}

impl Coordinates {
    #[cfg(test)]
    fn new<T, U>(group_id: T, artifact: U) -> Self
    where
        T: Into<String>,
        U: Into<String>,
    {
        Self {
            group_id: group_id.into(),
            artifact: artifact.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct VersionCheck {
    coordinates: Coordinates,
    versions: Vec<VersionReq>,
}
#[derive(Debug)]
struct CheckResult {
    coordinates: Coordinates,
    versions: Vec<(VersionReq, Option<Version>)>,
}
