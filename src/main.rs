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
//! Pre-releases can be included with the `--include-pre-releases` flag (or `-i` for short).
//!
//! The versions are matched in order and a single version can only be matched by one qualifier.
//! Previous matches will – depending on the range – consume all versions that would have also been matched by later qualifiers.
//! Try to define the qualifiers in the order from most restrictive to least.
//!
//! # Examples
//!
//! Matching against minor-compatible releases
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
//! Inclusion of pre releases.
//!
//!     $ latest-maven-version org.neo4j.gds:proc:~1.1:~1.3:1 --include-pre-releases
//!     Latest version(s) for org.neo4j.gds:proc:
//!     Latest version matching ~1.1: 1.1.4
//!     Latest version matching ~1.3: 1.3.1
//!     Latest version matching ^1: 1.4.0-alpha02
//!
//!
#[macro_use]
extern crate eyre;

use color_eyre::Help;
use eyre::{Context, Result};
use yansi::Paint;

fn main() -> Result<()> {
    let opts = opts::Opts::new()?;
    let versions = mvnmeta::check(opts.resolver(), opts.group_id(), opts.artifact())?;

    println!(
        "Latest version(s) for {}:{}:",
        Paint::magenta(opts.group_id()),
        Paint::blue(opts.artifact())
    );

    let latest = versions.latest_versions(opts.include_pre_releases(), opts.into_requirements());
    for (req, latest) in latest {
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

    Ok(())
}

mod opts {
    use super::*;
    use atty::Stream::Stdout;
    use clap::{
        AppSettings::{ColoredHelp, DeriveDisplayOrder, UnifiedHelpMessage},
        Clap,
    };
    use semver::VersionReq;

    #[derive(Clap, Debug)]
    #[clap(version, author, about, setting = ColoredHelp, setting = DeriveDisplayOrder, setting = UnifiedHelpMessage)]
    pub(crate) struct Opts {
        /// The maven groupId
        #[clap(short, long, requires = "artifact", conflicts_with = "coordinates")]
        group_id: Option<String>,

        /// The maven artifact
        #[clap(short, long, requires = "group-id", conflicts_with = "coordinates")]
        artifact: Option<String>,

        /// A list of version requirements that act as group identifiers
        ///
        /// Every matching version will be collected into the same bucket per requirement.
        /// The latest version per bucket is then shown.
        /// The value for a requirement follow the semver range specification from
        /// https://www.npmjs.com/package/semver#advanced-range-syntax
        #[clap(short, long, parse(try_from_str = parse_version))]
        versions: Vec<VersionReq>,

        /// Also consider pre releases.
        #[clap(short, long)]
        include_pre_releases: bool,

        /// Use this repository as resolver. Must follow maven style publication.
        ///
        #[clap(long, default_value = "https://repo.maven.apache.org/maven2")]
        resolver: String,

        /// Force colored output, even on non-terminals.
        #[clap(long, conflicts_with = "no-color", alias = "colour")]
        color: bool,

        /// Disabled colored output, even for terminals.
        #[clap(long, conflicts_with = "color", alias = "no-colour")]
        no_color: bool,

        /// The maven coordinates to check for. In the format of `{groupId}:{artifactId}[:{version}]`.
        #[clap(conflicts_with = "group-id", conflicts_with = "artifact")]
        coordinates: Option<String>,
    }

    fn parse_version(version: &str) -> Result<VersionReq> {
        VersionReq::parse(version)
        .wrap_err(format!("Could not parse {} into a semantic version range.", Paint::red(version)))
        .suggestion("Provide a valid range according to https://www.npmjs.com/package/semver#advanced-range-syntax")
    }

    impl Opts {
        pub(crate) fn new() -> Result<Self> {
            let mut opts = Opts::parse();

            if opts.disable_color() {
                Paint::disable();
            } else {
                color_eyre::install()?;
            }

            if let Some(coordinates) = opts.coordinates.take() {
                let mut segments = coordinates.split(':');
                opts.group_id = segments.next().map(String::from);
                if opts.group_id.is_none() {
                    bail!("The coordinates {} are invalid. Expected at least two elements, but got nothing.", Paint::red(coordinates));
                }

                opts.artifact = segments.next().map(String::from);
                if opts.artifact.is_none() {
                    bail!("The coordinates {} are invalid. Expected at least two elements, but got only one.", Paint::red(coordinates));
                }

                let versions = segments.map(parse_version).collect::<Result<Vec<_>>>()?;
                opts.versions.extend(versions);
            }

            Ok(opts)
        }

        fn disable_color(&self) -> bool {
            match (self.color, self.no_color) {
                (true, _) => false,
                (_, true) => true,
                _ => !atty::is(Stdout),
            }
        }

        pub(crate) fn group_id(&self) -> &str {
            self.group_id.as_deref().unwrap_or("org.neo4j")
        }

        pub(crate) fn artifact(&self) -> &str {
            self.artifact.as_deref().unwrap_or("neo4j")
        }

        pub(crate) fn resolver(&self) -> &str {
            &self.resolver
        }

        pub(crate) fn include_pre_releases(&self) -> bool {
            self.include_pre_releases
        }

        pub(crate) fn into_requirements(self) -> Vec<VersionReq> {
            self.versions
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

    pub(crate) fn check(resolver: &str, group_id: &str, artifact: &str) -> Result<Versions> {
        let url = url(resolver, group_id, artifact)
            .ok_or_else(|| eyre!("Invalid resolver: {}", Paint::red(resolver).bold()))?;
        let response = ureq::get(url.as_str())
            .timeout(Duration::from_secs(30))
            .call();
        if response.status() == 404 {
            Err(eyre!(
                "The coordinates {}:{} could not be found on the maven central server at {}",
                Paint::red(group_id).bold(),
                Paint::red(artifact).bold(),
                Paint::cyan(resolver)
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
    use itertools::Itertools;
    use semver::{Version, VersionReq};
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
                requirements.push(VersionReq::any());
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
