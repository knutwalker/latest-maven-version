use itertools::Itertools;
use semver::{Version, VersionReq};
use std::iter::FromIterator;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Versions {
    version: Vec<String>,
}

impl FromIterator<String> for Versions {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        let version = iter.into_iter().collect();
        Versions { version }
    }
}

impl<'a> FromIterator<&'a str> for Versions {
    fn from_iter<T: IntoIterator<Item = &'a str>>(iter: T) -> Self {
        let version = iter.into_iter().map(String::from).collect();
        Versions { version }
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
                VersionReq::parse("*").expect("Parsing `*` into a version range always succeeds.")
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
            .filter_map(|v| lenient_semver::parse::<Version>(v.as_str()).ok())
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
        for (pos, versions) in &versions_by_req {
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
        let version = items.into_iter().map(Into::into).collect_vec();
        Self { version }
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
    fn lenient_version_parsing() {
        let versions = Versions::from(["1.0.0", "1.337"].as_ref());
        assert_eq!(
            versions.find_latest_versions(&[VersionReq::any()], false),
            vec![Some(Version::new(1, 337, 0))]
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
