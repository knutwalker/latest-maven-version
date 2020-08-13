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
#[macro_use]
extern crate eyre;

use color_eyre::Help;
use eyre::{Context, Result};
use semver::{Version, VersionReq};
use yansi::Paint;

fn main() -> Result<()> {
    if atty::is(atty::Stream::Stdout) {
        color_eyre::install()?;
    } else {
        Paint::disable();
    }

    let mut opts = opts::Opts::new();
    let server = opts.resolver_server();
    let config = opts.config();
    let checks = opts.into_version_checks();

    let results = if checks.len() == 1 || config.jobs <= 1 {
        st_run(checks, server, config)?
    } else {
        mt_run(checks, server, config)?
    };

    for CheckResult {
        coordinates,
        versions,
    } in results
    {
        println!(
            "Latest version(s) for {}:{}:",
            Paint::magenta(coordinates.group_id),
            Paint::blue(coordinates.artifact)
        );

        for (req, latest) in versions {
            if let Some(latest) = latest {
                println!(
                    "Latest version matching {}: {}",
                    Paint::cyan(req).bold(),
                    Paint::green(latest).bold()
                );
            } else {
                println!("No version matching {}", Paint::yellow(req).bold());
            }
        }
    }

    Ok(())
}

fn st_run(checks: Vec<VersionCheck>, server: Server, config: Config) -> Result<Vec<CheckResult>> {
    let results = checks
        .into_iter()
        .map(|check| run_check(check, &server, config.include_pre_releases))
        .collect::<Result<Vec<_>>>()?;

    Ok(results)
}

fn mt_run(checks: Vec<VersionCheck>, server: Server, config: Config) -> Result<Vec<CheckResult>> {
    use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Arc,
        },
        thread,
    };

    let spinner_style = ProgressStyle::default_spinner()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
        .template("{prefix:.bold.dim} {spinner} {wide_msg}");

    let total = checks.len();
    let threads = total.min(config.jobs);

    let mut slots = vec![vec![]; threads];
    for (i, check) in checks.into_iter().enumerate() {
        let bucket = i % threads;
        slots[bucket].push(check);
    }

    let (sender, results) = mpsc::channel::<Result<CheckResult>>();

    let current = Arc::new(AtomicUsize::new(0));
    let server = Arc::new(server);
    let m = MultiProgress::new();

    for checks in slots {
        let pb = m.add(ProgressBar::new(total as u64));
        pb.set_style(spinner_style.clone());
        let server = Arc::clone(&server);
        let current = Arc::clone(&current);
        let sender = sender.clone();

        let _ = thread::spawn(move || {
            for check in checks {
                let i = current.fetch_add(1, Ordering::SeqCst);
                pb.set_prefix(&format!("[{}/{}]", i + 1, total));
                pb.set_message(&format!(
                    "{}:{}",
                    Paint::magenta(&check.coordinates.group_id),
                    Paint::blue(&check.coordinates.artifact)
                ));
                pb.inc(1);
                let result = run_check(check, &*server, config.include_pre_releases);
                if sender.send(result).is_err() {
                    break;
                }
            }
            pb.finish_with_message("waiting...");
        });
    }

    m.join_and_clear()?;

    let results = results.try_iter().collect::<Result<Vec<_>>>()?;
    Ok(results)
}

fn run_check(
    check: VersionCheck,
    server: &Server,
    include_pre_releases: bool,
) -> Result<CheckResult> {
    let VersionCheck {
        coordinates,
        versions,
    } = check;

    let all_versions = mvnmeta::check(server, &coordinates.group_id, &coordinates.artifact)?;
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
    jobs: usize,
}

#[derive(Debug, Clone)]
struct Coordinates {
    group_id: String,
    artifact: String,
}

#[derive(Debug, Clone)]
struct VersionCheck {
    coordinates: Coordinates,
    versions: Vec<VersionReq>,
}
#[derive(Debug)]
struct CheckResult {
    coordinates: Coordinates,
    versions: Vec<(VersionReq, Option<Version>)>,
}

mod opts {
    use super::*;
    use clap::{
        AppSettings::{ArgRequiredElseHelp, ColoredHelp, DeriveDisplayOrder, UnifiedHelpMessage},
        Clap,
    };
    use std::num::NonZeroUsize;

    #[derive(Clap, Debug)]
    #[clap(version, author, about, setting = ArgRequiredElseHelp, setting = ColoredHelp, setting = DeriveDisplayOrder, setting = UnifiedHelpMessage)]
    pub(crate) struct Opts {
        /// The maven coordinates to check for. Can be specified multiple times.
        ///
        /// These arguments take the form of `{groupId}:{artifactId}[:{version}]*`.
        /// The versions are treated as requirement qualifiers.
        /// Every matching version will be collected into the same bucket per requirement.
        /// The latest version per bucket is then shown.
        /// The value for a requirement follow the semver range specification from
        /// https://www.npmjs.com/package/semver#advanced-range-syntax
        #[clap(required = true, min_values = 1, parse(try_from_str = parse_coordinates))]
        version_checks: Vec<VersionCheck>,

        /// Also consider pre releases.
        #[clap(short, long)]
        include_pre_releases: bool,

        /// Use this repository as resolver. Must follow maven style publication.
        ///
        /// By default, Maven Central is used.
        #[clap(short, long, alias = "repo")]
        resolver: Option<String>,

        /// Username for authentication against the resolver. Aliased to --username.
        ///
        /// If provided, requests against the resolver will authenticate with Basic Auth.
        /// See the `--help` option for the password to see how to provide the password.
        #[clap(short, long, alias = "username")]
        user: Option<String>,

        /// Consider leaving this undefined, the password will be read from stdin. Aliased to --password.
        ///
        /// Password for authentication against the resolver. If provided, the given value is used.
        /// However, if not provided, but a username has been, the password will be read from a secure prompt.
        #[clap(short, long, requires = "user", alias = "password")]
        pass: Option<String>,

        /// When multiple coordinates are given, query at most <jobs> at once. Defaults to the number of physical CPU cores.
        #[clap(short, long)]
        jobs: Option<NonZeroUsize>,
    }

    fn parse_coordinates(input: &str) -> Result<VersionCheck> {
        let mut segments = input.split(':');
        let group_id = match segments.next() {
            Some(group_id) => String::from(group_id),
            None => bail!(
                "The coordinates {} are invalid. Expected at least two elements, but got nothing.",
                Paint::red(input)
            ),
        };
        let artifact = match segments.next() {
            Some(artifact_id) => String::from(artifact_id),
            None => bail!(
                "The coordinates {} are invalid. Expected at least two elements, but got only one.",
                Paint::red(input)
            ),
        };

        let versions = segments.map(parse_version).collect::<Result<Vec<_>>>()?;
        Ok(VersionCheck {
            coordinates: Coordinates { group_id, artifact },
            versions,
        })
    }

    fn parse_version(version: &str) -> Result<VersionReq> {
        VersionReq::parse(version)
            .wrap_err(format!("Could not parse {} into a semantic version range.", Paint::red(version)))
            .suggestion("Provide a valid range according to https://www.npmjs.com/package/semver#advanced-range-syntax")
    }

    impl Opts {
        pub(crate) fn new() -> Self {
            Opts::parse()
        }

        pub(crate) fn resolver_server(&mut self) -> Server {
            let url = self
                .resolver
                .take()
                .unwrap_or_else(|| String::from("https://repo.maven.apache.org/maven2"));
            let auth = self.auth();
            Server { url, auth }
        }

        fn auth(&mut self) -> Option<(String, String)> {
            let user = self.user.take()?;
            let pass = match self.pass.take() {
                Some(pass) => pass,
                None => {
                    use dialoguer::Password;
                    Password::new()
                        .with_prompt(format!("Password for {}", Paint::cyan(&user)))
                        .allow_empty_password(true)
                        .interact()
                        .ok()?
                }
            };

            Some((user, pass))
        }

        pub(crate) fn config(&self) -> Config {
            Config {
                include_pre_releases: self.include_pre_releases,
                jobs: self.jobs(),
            }
        }

        fn jobs(&self) -> usize {
            self.jobs
                .map(|jobs| jobs.get())
                .unwrap_or_else(num_cpus::get_physical)
        }

        pub(crate) fn into_version_checks(self) -> Vec<VersionCheck> {
            self.version_checks
        }
    }
}

mod mvnmeta {
    use super::{versions::Versions, *};
    use serde::Deserialize;
    use serde_xml_rs as xml;
    use std::time::Duration;
    use url::Url;

    #[derive(Debug, Deserialize)]
    struct MetaData {
        versioning: Versioning,
    }

    #[derive(Debug, Deserialize)]
    struct Versioning {
        versions: Versions,
    }

    pub(crate) fn check(server: &Server, group_id: &str, artifact: &str) -> Result<Versions> {
        let url = url(&server.url, group_id, artifact)
            .ok_or_else(|| eyre!("Invalid resolver: {}", Paint::red(&server.url).bold()))?;

        let mut request = ureq::get(url.as_str());
        if let Some((user, pass)) = &server.auth {
            request.auth(user, pass);
        }

        let response = request.timeout(Duration::from_secs(30)).call();
        if response.status() == 404 {
            Err(eyre!(
                "The coordinates {}:{} could not be found on the maven central server at {}",
                Paint::red(group_id).bold(),
                Paint::red(artifact).bold(),
                Paint::cyan(&server.url)
            ))
            .suggestion("Provide existing coordinates.")?;
        }
        if response.error() {
            let client_error = response.client_error();
            let body = response
                .into_string()
                .wrap_err("Could not read the error response from maven central.")
                .suggestion(
                    "Maybe your internet connection is gone. Maven central could also be down.",
                )?;

            let err = Err(eyre!("{}", body))
                .wrap_err("Could not read Maven metadata from maven central.");

            return if client_error {
                err.suggestion(
                    "There is likely something wrong with your request, please check your inputs.",
                )
            } else {
                err.suggestion(
                    "There is likely something wrong with maven central. Please try again later.",
                )
            };
        }

        let meta_data: MetaData = xml::from_reader(response.into_reader())
            .wrap_err("Unable to read Maven metadata into XML format.")
            .suggestion("try using a file that exists next time")?;
        let versions = meta_data.versioning.versions;
        Ok(versions)
    }

    fn url(resolver: &str, group_id: &str, artifact: &str) -> Option<Url> {
        let mut url = Url::parse(resolver).ok()?;

        url.path_segments_mut()
            .ok()?
            .extend(group_id.split('.'))
            .push(artifact)
            .push("maven-metadata.xml");

        Some(url)
    }
}

mod versions {
    use super::*;
    use itertools::Itertools;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub(crate) struct Versions {
        version: Vec<String>,
    }

    #[cfg(test)]
    impl From<&str> for Versions {
        fn from(version: &str) -> Self {
            let version = vec![version.to_string()];
            Self { version }
        }
    }

    #[cfg(test)]
    impl<T> From<&[T]> for Versions
    where
        T: ToString,
    {
        fn from(items: &[T]) -> Self {
            let version = items.iter().map(|x| x.to_string()).collect_vec();
            Self { version }
        }
    }

    #[cfg(test)]
    impl<T> From<Vec<T>> for Versions
    where
        T: Into<String>,
    {
        fn from(items: Vec<T>) -> Self {
            let version = items.into_iter().map(|x| x.into()).collect_vec();
            Self { version }
        }
    }

    impl Versions {
        pub(crate) fn latest_versions(
            &self,
            allow_pre_release: bool,
            mut requirements: Vec<VersionReq>,
        ) -> Vec<(VersionReq, Option<Version>)> {
            if requirements.is_empty() {
                let req = if allow_pre_release {
                    VersionReq::any()
                } else {
                    VersionReq::parse("*")
                        .expect("Parsing `*` into a version range always succeeds.")
                };
                requirements.push(req);
            }
            let latest = self.find_latest_versions(&requirements[..], allow_pre_release);
            requirements.into_iter().zip(latest.into_iter()).collect()
        }

        fn find_latest_versions(
            &self,
            requirements: &[VersionReq],
            allow_pre_release: bool,
        ) -> Vec<Option<Version>> {
            let versions_by_req = self
                .version
                .iter()
                .filter_map(|v| Version::parse(v.as_str()).ok())
                .filter_map(|v| {
                    if allow_pre_release {
                        let version = Version::new(v.major, v.minor, v.patch);
                        requirements
                            .iter()
                            .position(|r| r.matches(&version))
                            .map(|p| (p, v))
                    } else {
                        requirements
                            .iter()
                            .position(|r| r.matches(&v))
                            .map(|p| (p, v))
                    }
                })
                .group_by(|(idx, _)| *idx);

            let mut latest = vec![None; requirements.len()];
            for (pos, versions) in versions_by_req.into_iter() {
                let new = versions.map(|(_, vs)| vs).max();
                match &mut latest[pos] {
                    Some(v1) => match new {
                        Some(v2) if v2 > *v1 => {
                            *v1 = v2;
                        }
                        _ => {}
                    },
                    None => latest[pos] = new,
                }
            }

            latest
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_empty_reqs() {
            let versions = Versions::from("1.0.0");
            assert_eq!(versions.find_latest_versions(&[], false), vec![]);
        }

        #[test]
        fn test_empty_versions() {
            let versions = Versions::from(Vec::<String>::new());
            assert_eq!(
                versions.find_latest_versions(&[VersionReq::any()], false),
                vec![None]
            );
        }

        #[test]
        fn match_single_version() {
            let versions = Versions::from("1.0.0");
            assert_eq!(
                versions.find_latest_versions(&[VersionReq::any()], false),
                vec![Some(Version::new(1, 0, 0))]
            );
        }

        #[test]
        fn select_latest() {
            let versions = Versions::from(["1.0.0", "1.3.37"].as_ref());
            assert_eq!(
                versions.find_latest_versions(&[VersionReq::any()], false),
                vec![Some(Version::new(1, 3, 37))]
            );
        }

        #[test]
        fn ignore_wrong_versions() {
            let versions = Versions::from(["1.0.0", "1.337"].as_ref());
            assert_eq!(
                versions.find_latest_versions(&[VersionReq::any()], false),
                vec![Some(Version::new(1, 0, 0))]
            );
        }

        #[test]
        fn group_on_reqs() {
            let versions = Versions::from(["1.0.0", "1.2.3", "2.0.0", "2.1337.42"].as_ref());
            assert_eq!(
                versions.find_latest_versions(
                    &[
                        VersionReq::parse("1.x").unwrap(),
                        VersionReq::parse("2.x").unwrap()
                    ],
                    false
                ),
                vec![Some(Version::new(1, 2, 3)), Some(Version::new(2, 1337, 42))]
            );
        }

        #[test]
        fn skip_unmatched_reqs() {
            let versions = Versions::from(["1.0.0", "2.0.0"].as_ref());
            assert_eq!(
                versions.find_latest_versions(
                    &[
                        VersionReq::parse("1.x").unwrap(),
                        VersionReq::parse("42.x").unwrap(),
                        VersionReq::parse("2.x").unwrap()
                    ],
                    false
                ),
                vec![
                    Some(Version::new(1, 0, 0)),
                    None,
                    Some(Version::new(2, 0, 0))
                ]
            );
        }

        #[test]
        fn skip_overshadowed_reqs() {
            let versions = Versions::from(["1.0.42", "1.2.3"].as_ref());
            assert_eq!(
                versions.find_latest_versions(
                    &[
                        VersionReq::parse("^1").unwrap(),
                        VersionReq::parse("1.2.3").unwrap(),
                    ],
                    false
                ),
                vec![Some(Version::new(1, 2, 3)), None,]
            );
        }

        #[test]
        fn skip_prerelease() {
            let versions = Versions::from(["1.0.0", "1.1.0-alpha01"].as_ref());
            assert_eq!(
                versions.find_latest_versions(&[VersionReq::parse("^1").unwrap(),], false),
                vec![Some(Version::new(1, 0, 0))]
            );
        }

        #[test]
        fn include_prerelease() {
            let versions = Versions::from(["1.0.0", "1.1.0-alpha01"].as_ref());
            assert_eq!(
                versions.find_latest_versions(&[VersionReq::parse("^1").unwrap(),], true),
                vec![Some(Version::parse("1.1.0-alpha01").unwrap())]
            );
        }
    }
}
