use crate::{Config, Coordinates, Server, VersionCheck};
use clap::{
    AppSettings::{ArgRequiredElseHelp, ColoredHelp, DeriveDisplayOrder, UnifiedHelpMessage},
    Clap,
};
use console::style;
use semver::{ReqParseError, VersionReq};
use std::fmt::Display;

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
    #[cfg(feature = "parallel")]
    #[cfg_attr(feature = "parallel", clap(short, long))]
    jobs: Option<std::num::NonZeroUsize>,
}

#[non_exhaustive]
#[derive(Debug)]
pub(crate) enum Error {
    MissingGroupId(String),
    MissingArtifact(String),
    InvalidRange(String, ReqParseError),
}

fn parse_coordinates(input: &str) -> Result<VersionCheck, Error> {
    let mut segments = input.split(':');
    let group_id = match segments.next() {
        Some(group_id) => String::from(group_id),
        None => return Err(Error::MissingGroupId(input.into())),
    };
    let artifact = match segments.next() {
        Some(artifact_id) => String::from(artifact_id),
        None => return Err(Error::MissingArtifact(input.into())),
    };

    let versions = segments.map(parse_version).collect::<Result<Vec<_>, _>>()?;
    Ok(VersionCheck {
        coordinates: Coordinates { group_id, artifact },
        versions,
    })
}

fn parse_version(version: &str) -> Result<VersionReq, Error> {
    VersionReq::parse(version).map_err(|e| Error::InvalidRange(version.into(), e))
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
                    .with_prompt(format!("Password for {}", style(&user).cyan()))
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
            #[cfg(feature = "parallel")]
            jobs: self.jobs(),
        }
    }

    #[cfg(feature = "parallel")]
    fn jobs(&self) -> usize {
        self.jobs
            .map(|jobs| jobs.get())
            .unwrap_or_else(num_cpus::get_physical)
    }

    pub(crate) fn into_version_checks(self) -> Vec<VersionCheck> {
        self.version_checks
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::MissingGroupId(input) => write!(
                f,
                "The coordinates {} are invalid. Expected at least two elements, but got nothing.",
                style(input).red()
            ),
            Error::MissingArtifact(input) => write!(
                f,
                "The coordinates {} are invalid. Expected at least two elements, but got only one.",
                style(input).red()
            ),
            Error::InvalidRange(input, _) => write!(
                f,
                "Could not parse {} into a semantic version range. Please provide a valid range according to {}",
                style(input).red(),
                style("https://www.npmjs.com/package/semver#advanced-range-syntax").cyan().underlined(),
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Error::InvalidRange(_, src) = self {
            Some(src)
        } else {
            None
        }
    }
}
