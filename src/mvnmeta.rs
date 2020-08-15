use crate::{Coordinates, Server, Versions};
use console::style;
use serde::Deserialize;
use serde_xml_rs as xml;
use std::{fmt::Display, time::Duration};
use url::Url;

#[non_exhaustive]
#[derive(Debug)]
pub(crate) enum Error {
    InvalidResolver(String),
    CoordinatesNotFound(Coordinates, String),
    ClientError(String, ErrorResponse),
    ServerError(String, ErrorResponse),
    ErrorWhileReadingError(std::io::Error),
    ParseXmlError(xml::Error),
}

#[non_exhaustive]
#[derive(Debug)]
pub(crate) struct ErrorResponse(String);

pub(super) fn check(server: &Server, group_id: &str, artifact: &str) -> Result<Versions, Error> {
    let url = url(&server.url, group_id, artifact)
        .ok_or_else(|| Error::InvalidResolver(server.url.clone()))?;

    let mut request = ureq::get(url.as_str());
    if let Some((user, pass)) = &server.auth {
        request.auth(user, pass);
    }

    let response = request.timeout(Duration::from_secs(30)).call();
    if response.status() == 404 {
        return Err(Error::CoordinatesNotFound(
            Coordinates {
                group_id: group_id.into(),
                artifact: artifact.into(),
            },
            server.url.clone(),
        ));
    }
    if response.error() {
        let client_error = response.client_error();
        let body = response.into_string()?;

        let err = if client_error {
            Error::ClientError(server.url.clone(), ErrorResponse(body))
        } else {
            Error::ServerError(server.url.clone(), ErrorResponse(body))
        };
        return Err(err);
    }

    let meta_data: MetaData = xml::from_reader(response.into_reader())?;
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

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::ErrorWhileReadingError(source)
    }
}

impl From<xml::Error> for Error {
    fn from(source: xml::Error) -> Self {
        Error::ParseXmlError(source)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidResolver(url) => write!(
                f,
                "The resolver {} is an invalid URL.",
                style(url).red().bold()
            ),
            Error::CoordinatesNotFound(coords, url) => write!(
                f,
                "The coordinates {}:{} could not be found using the resolver {}",
                style(&coords.group_id).red().bold(),
                style(&coords.artifact).red().bold(),
                style(url).cyan()
            ),
            Error::ClientError(url, _) => write!(
                f,
                "Could not read Maven metadata using the resolver {}",
                style(url).cyan()
            ),
            Error::ServerError(url, _) => write!(
                f,
                "Could not read Maven metadata using the resolver {}",
                style(url).cyan()
            ),
            Error::ErrorWhileReadingError(_) => {
                write!(f, "Could not read the error response from maven central.")
            }
            Error::ParseXmlError(_) => write!(f, "Unable to parse Maven metadata XML file."),
        }
    }
}

impl Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ClientError(_, src) => Some(src),
            Error::ServerError(_, src) => Some(src),
            Error::ErrorWhileReadingError(src) => Some(src),
            Error::ParseXmlError(src) => Some(src),
            _ => None,
        }
    }
}

impl std::error::Error for ErrorResponse {}

#[derive(Debug, Deserialize)]
struct MetaData {
    versioning: Versioning,
}

#[derive(Debug, Deserialize)]
struct Versioning {
    versions: Versions,
}
