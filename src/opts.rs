use crate::{Config, Coordinates, Server, VersionCheck};
use clap::Parser;
use console::style;
use semver::{Error as ReqParseError, VersionReq};
use std::fmt::Display;

#[derive(Parser, Debug)]
#[cfg_attr(test, derive(Default))]
#[command(version, about, arg_required_else_help = true)]
pub(crate) struct Opts {
    /// The maven coordinates to check for. Can be specified multiple times.
    ///
    /// These arguments take the form of `{groupId}:{artifactId}[:{version}]*`.
    /// The versions are treated as requirement qualifiers.
    /// Every matching version will be collected into the same bucket per requirement.
    /// The latest version per bucket is then shown.
    /// The value for a requirement follow the semver range specification from
    /// https://www.npmjs.com/package/semver#advanced-range-syntax
    #[arg(num_args = 1.., value_parser(parse_coordinates), allow_negative_numbers = true)]
    version_checks: Vec<VersionCheck>,

    /// Also consider pre releases.
    #[arg(short, long)]
    include_pre_releases: bool,

    /// Use this repository as resolver.
    ///
    /// This repository must follow maven style publication.
    /// By default, Maven Central is used.
    #[arg(short, long, alias = "repo")]
    resolver: Option<String>,

    /// Username for authentication against the resolver.
    ///
    /// If provided, requests against the resolver will authenticate with Basic Auth.
    /// The password for this user will be read from stdin.
    #[arg(short, long, alias = "username")]
    user: Option<String>,

    /// Consider leaving this undefined, the password will be read from stdin.
    ///
    /// Password for authentication against the resolver. If provided, the given value is used.
    /// However, if not provided, but a username has been, the password will be read from a secure prompt.
    #[arg(long, requires = "user")]
    insecure_password: Option<String>,
}

#[non_exhaustive]
#[derive(Debug)]
pub(crate) enum Error {
    EmptyGroupId(String),
    EmptyArtifact(String),
    MissingArtifact(String),
    InvalidRange(String, ReqParseError),
}

fn parse_coordinates(input: &str) -> Result<VersionCheck, Error> {
    let mut segments = input.split(':').map(str::trim);
    let group_id = match segments.next() {
        Some(group_id) if !group_id.is_empty() => String::from(group_id),
        _ => return Err(Error::EmptyGroupId(input.into())),
    };
    let artifact = match segments.next() {
        Some(artifact_id) if !artifact_id.is_empty() => String::from(artifact_id),
        Some(_) => return Err(Error::EmptyArtifact(input.into())),
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

static MAVEN_CENTRAL: &str = "https://repo.maven.apache.org/maven2";

impl Opts {
    pub(crate) fn new() -> Self {
        Opts::parse()
    }

    #[cfg(test)]
    fn of(args: &[&str]) -> Result<Self, clap::Error> {
        let mut args = args.to_vec();
        args.insert(0, "binary-name");
        Opts::try_parse_from(args)
    }

    pub(crate) fn resolver_server(&mut self) -> Server {
        let url = self
            .resolver
            .take()
            .unwrap_or_else(|| String::from(MAVEN_CENTRAL));
        let auth = self.auth();
        Server { url, auth }
    }

    fn auth(&mut self) -> Option<(String, String)> {
        let user = self.user.take()?;
        let pass = match self.insecure_password.take() {
            Some(pass) => pass,
            None => Self::ask_pass(&user)?,
        };

        Some((user, pass))
    }

    #[cfg(not(test))]
    fn ask_pass(user: &str) -> Option<String> {
        let prompt = format!("Enter password for [{}]: ", style(user).cyan());
        rpassword::prompt_password(prompt).ok()
    }

    #[cfg(test)]
    fn ask_pass(user: &str) -> Option<String> {
        let user = format!("{}\n", user);
        let mut cursor = std::io::Cursor::new(user);
        rpassword::read_password_from_bufread(&mut cursor).ok()
    }

    pub(crate) fn config(&self) -> Config {
        Config {
            include_pre_releases: self.include_pre_releases,
        }
    }

    pub(crate) fn into_version_checks(self) -> Vec<VersionCheck> {
        self.version_checks
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EmptyGroupId(input) => write!(
                f,
                "The groupId may not be empty in {}",
                style(input).red().bold()
            ),
            Error::EmptyArtifact(input) => write!(
                f,
                "The artifact may not be empty in {}",
                style(input).red().bold()
            ),
            Error::MissingArtifact(input) => write!(
                f,
                "The artifact is missing in {}",
                style(input).red().bold()
            ),
            Error::InvalidRange(input, _) => write!(
                f,
                "Could not parse {} into a semantic version range. Please provide a valid range according to {}",
                style(input).red().bold(),
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

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::EmptyGroupId(lhs), Self::EmptyGroupId(rhs)) => lhs == rhs,
            (Self::EmptyArtifact(lhs), Self::EmptyArtifact(rhs)) => lhs == rhs,
            (Self::MissingArtifact(lhs), Self::MissingArtifact(rhs)) => lhs == rhs,
            (Self::InvalidRange(lhs, _), Self::InvalidRange(rhs, _)) => lhs == rhs,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::{ContextKind, ContextValue, ErrorKind};
    use test_case::test_case;

    #[test]
    fn empty_args_shows_help() {
        let err = Opts::of(&[]).unwrap_err();
        assert_eq!(
            err.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn test_empty_version_arg() {
        console::set_colors_enabled(false);
        let err = Opts::of(&[""]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);

        let arg = ContextValue::String("[VERSION_CHECKS]...".into());
        let value = ContextValue::String("".into());

        let expected = vec![
            (ContextKind::InvalidArg, &arg),
            (ContextKind::InvalidValue, &value),
        ];

        let context = err.context().collect::<Vec<_>>();
        assert_eq!(context, expected);
    }

    #[test_case("foo:bar", "foo", "bar"; "case1")]
    #[test_case("foo.bar:baz", "foo.bar", "baz"; "case2")]
    #[test_case("foo:bar.baz", "foo", "bar.baz"; "case3")]
    #[test_case("foo.bar:baz.qux", "foo.bar", "baz.qux"; "case4")]
    #[test_case("42:1337", "42", "1337"; "case5")]
    #[test_case(" 42 :  1337  ", "42", "1337"; "case6")]
    fn test_version_arg_coords(arg: &str, group_id: &str, artifact: &str) {
        let opts = Opts::of(&[arg]).unwrap();
        let mut checks = opts.version_checks.into_iter();
        let check = checks.next().unwrap();
        assert_eq!(check.coordinates.group_id, group_id);
        assert_eq!(check.coordinates.artifact, artifact);
        assert_eq!(checks.next(), None);
    }

    #[test_case(":foo" => Error::EmptyGroupId(":foo".into()); "empty_group_id_1")]
    #[test_case(":foo:" => Error::EmptyGroupId(":foo:".into()); "empty_group_id_2")]
    #[test_case("" => Error::EmptyGroupId("".into()); "empty_group_id_3")]
    #[test_case(":" => Error::EmptyGroupId(":".into()); "empty_group_id_4")]
    #[test_case("::" => Error::EmptyGroupId("::".into()); "empty_group_id_5")]
    #[test_case("  " => Error::EmptyGroupId("  ".into()); "empty_group_id_6")]
    #[test_case("  :" => Error::EmptyGroupId("  :".into()); "empty_group_id_7")]
    #[test_case("foo:" => Error::EmptyArtifact("foo:".into()); "empty_artifact_1")]
    #[test_case("foo::" => Error::EmptyArtifact("foo::".into()); "empty_artifact_2")]
    #[test_case("foo: " => Error::EmptyArtifact("foo: ".into()); "empty_artifact_3")]
    #[test_case("foo: :" => Error::EmptyArtifact("foo: :".into()); "empty_artifact_4")]
    #[test_case("foo" => Error::MissingArtifact("foo".into()); "missing_artifact")]
    fn test_invalid_coords(arg: &str) -> Error {
        parse_coordinates(arg).unwrap_err()
    }

    #[test_case(":foo"; "empty_group_id_1")]
    #[test_case(":foo:"; "empty_group_id_2")]
    #[test_case(":"; "empty_group_id_4")]
    #[test_case("::"; "empty_group_id_5")]
    #[test_case("  "; "empty_group_id_6")]
    #[test_case("  :"; "empty_group_id_7")]
    #[test_case("foo:"; "empty_artifact_1")]
    #[test_case("foo::"; "empty_artifact_2")]
    #[test_case("foo: "; "empty_artifact_3")]
    #[test_case("foo: :"; "empty_artifact_4")]
    #[test_case("foo"; "missing_artifact")]
    fn test_version_arg_invalid_coords(arg: &str) {
        console::set_colors_enabled(false);
        let err = Opts::of(&[arg]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);

        let value = ContextValue::String(arg.into());
        let arg = ContextValue::String("[VERSION_CHECKS]...".into());

        let expected = vec![
            (ContextKind::InvalidArg, &arg),
            (ContextKind::InvalidValue, &value),
        ];

        let context = err.context().collect::<Vec<_>>();
        assert_eq!(context, expected);
    }

    #[test_case("foo:bar:1", vec!["1"]; "version 1")]
    #[test_case("foo:bar:0", vec!["0"]; "version 0")]
    #[test_case("foo:bar:*", vec!["*"]; "any version")]
    #[test_case("foo:bar:", vec!["*"] => inconclusive; "empty version")]
    #[test_case("foo:bar", vec![]; "no version")]
    #[test_case("foo:bar:1.0", vec!["1.0"]; "version 1.0")]
    #[test_case("foo:bar:1.x", vec!["1.x"]; "version 1.x")]
    #[test_case("foo:bar:1.*", vec!["1.*"]; "version 1.*")]
    #[test_case("foo:bar:=1.2.3", vec!["=1.2.3"]; "exact version")]
    #[test_case("foo:bar:<1.2.3", vec!["<1.2.3"]; "lt version")]
    #[test_case("foo:bar:>1.2.3", vec![">1.2.3"]; "gt version")]
    #[test_case("foo:bar:<=1.2.3", vec!["<=1.2.3"]; "lte version")]
    #[test_case("foo:bar:>=1.2.3", vec![">=1.2.3"]; "gte version")]
    #[test_case("foo:bar:1.2.3 2", vec!["1.2.3 2"] => inconclusive; "multi range with space")]
    #[test_case("foo:bar:1.2.3||2", vec!["1.2.3||2"] => inconclusive; "multi range with or")]
    #[test_case("foo:bar:1.2.3:2", vec!["1.2.3", "2"]; "multiple ranges")]
    fn test_version_arg_range(arg: &str, ranges: Vec<&str>) {
        let ranges = ranges
            .into_iter()
            .map(VersionReq::parse)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let opts = Opts::of(&[arg]).unwrap();
        let mut checks = opts.version_checks.into_iter();
        let check = checks.next().unwrap();
        assert_eq!(check.versions, ranges);
        assert_eq!(checks.next(), None);
    }

    #[test_case("foo:bar:01"; "major with leading 0")]
    #[test_case("foo:bar:1.02"; "minor with leading 0")]
    #[test_case("foo:bar:."; "missing major")]
    #[test_case("foo:bar:1."; "trailing period before minor")]
    #[test_case("foo:bar:1.."; "two trailing periods")]
    #[test_case("foo:bar:1.2."; "trailing period before path")]
    #[test_case("foo:bar:qux"; "non numeric major")]
    #[test_case("foo:bar:1.qux"; "non numeric minor")]
    #[test_case("foo:bar:-42"; "negative major")]
    #[test_case("foo:bar:*42"; "mixed star and version")]
    #[test_case("foo:bar:1.3.3.7"; "4 segments")]
    #[test_case("foo:bar:1:foo"; "second version fails")]
    #[test_case("foo:bar:1.2.3,2" => inconclusive; "multi range with comma separator")]
    fn test_version_arg_invalid_range(arg: &str) {
        console::set_colors_enabled(false);
        let err = Opts::of(&[arg]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);

        let value = ContextValue::String(arg.into());
        let arg = ContextValue::String("[VERSION_CHECKS]...".into());

        let expected = vec![
            (ContextKind::InvalidArg, &arg),
            (ContextKind::InvalidValue, &value),
        ];

        let context = err.context().collect::<Vec<_>>();
        assert_eq!(context, expected);
    }

    #[test]
    fn test_default_pre_release_flag() {
        let opts = Opts::default();
        assert_eq!(opts.include_pre_releases, false);
        assert_eq!(opts.config().include_pre_releases, false);
    }

    #[test_case("-i"; "short flag")]
    #[test_case("--include-pre-releases"; "long flag")]
    fn test_pre_release_flag(flag: &str) {
        let opts = Opts::of(&[flag]).unwrap();
        assert_eq!(opts.include_pre_releases, true);
        assert_eq!(opts.config().include_pre_releases, true);
    }

    #[test]
    fn test_default_resolver() {
        let mut opts = Opts::default();
        assert_eq!(opts.resolver, None);
        assert_eq!(opts.resolver_server().url, MAVEN_CENTRAL);
    }

    #[test_case("-r"; "short option")]
    #[test_case("--resolver"; "long option")]
    #[test_case("--repo"; "alias")]
    fn test_resolver_option(flag: &str) {
        let mut opts = Opts::of(&[flag, "Server"]).unwrap();
        assert_eq!(opts.resolver, Some("Server".into()));
        assert_eq!(opts.resolver_server().url, "Server");
    }

    #[test_case("-r"; "short option")]
    #[test_case("--resolver"; "long option")]
    #[test_case("--repo"; "alias")]
    fn test_resolver_missing_value(flag: &str) {
        let err = Opts::of(&[flag]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);

        let expected = vec![
            (
                ContextKind::InvalidArg,
                ContextValue::String("--resolver <RESOLVER>".into()),
            ),
            (
                ContextKind::InvalidValue,
                ContextValue::String(String::new()),
            ),
            (ContextKind::ValidValue, ContextValue::Strings(Vec::new())),
        ];

        let context = err
            .context()
            .map(|(k, v)| (k, v.clone()))
            .collect::<Vec<_>>();
        assert_eq!(context, expected);
    }

    #[test]
    fn test_default_auth() {
        let mut opts = Opts::default();
        assert_eq!(opts.user, None);
        assert_eq!(opts.insecure_password, None);
        assert_eq!(opts.resolver_server().auth, None);
    }

    #[test_case("-u"; "short option")]
    #[test_case("--user"; "long option")]
    #[test_case("--username"; "alias")]
    fn test_user_option(flag: &str) {
        let mut opts = Opts::of(&[flag, "Alice"]).unwrap();
        assert_eq!(opts.user.as_deref(), Some("Alice"));
        assert_eq!(opts.resolver_server().auth.unwrap().0, "Alice");
    }

    #[test_case("-u"; "short option")]
    #[test_case("--user"; "long option")]
    #[test_case("--username"; "alias")]
    fn test_user_missing_value(flag: &str) {
        let err = Opts::of(&[flag]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);

        let expected = vec![
            (
                ContextKind::InvalidArg,
                ContextValue::String("--user <USER>".into()),
            ),
            (
                ContextKind::InvalidValue,
                ContextValue::String(String::new()),
            ),
            (ContextKind::ValidValue, ContextValue::Strings(Vec::new())),
        ];

        let context = err
            .context()
            .map(|(k, v)| (k, v.clone()))
            .collect::<Vec<_>>();
        assert_eq!(context, expected);
    }

    #[test]
    fn test_password_option() {
        let mut opts = Opts::of(&["--user", "Alice", "--insecure-password", "s3cure"]).unwrap();
        assert_eq!(opts.insecure_password, Some("s3cure".into()));
        assert_eq!(opts.resolver_server().auth.unwrap().1, "s3cure");
    }

    #[test]
    fn test_password_option_requires_user() {
        let err = Opts::of(&["--insecure-password", "s3cure"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn test_password_missing_value() {
        let err = Opts::of(&["--user", "Alice", "--insecure-password"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidValue);

        let expected = vec![
            (
                ContextKind::InvalidArg,
                ContextValue::String("--insecure-password <INSECURE_PASSWORD>".into()),
            ),
            (
                ContextKind::InvalidValue,
                ContextValue::String(String::new()),
            ),
            (ContextKind::ValidValue, ContextValue::Strings(Vec::new())),
        ];

        let context = err
            .context()
            .map(|(k, v)| (k, v.clone()))
            .collect::<Vec<_>>();
        assert_eq!(context, expected);
    }
}
