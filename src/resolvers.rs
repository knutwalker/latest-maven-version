use crate::{Coordinates, Versions};
use console::style;
use serde::Deserialize;
use serde_xml_rs as xml;
use std::{fmt::Display, io::Read, time::Duration};
use url::Url;

pub(crate) trait Resolver {
    fn resolve<T: Client>(&self, coordinates: &Coordinates, client: &T) -> Result<Versions, Error>;
}

#[derive(Debug)]
pub(crate) enum Error {
    CoordinatesNotFound {
        coordinates: Coordinates,
        server: String,
        url: Url,
    },
    ClientError(String, ErrorResponse),
    ServerError(String, ErrorResponse),
    ErrorWhileReadingError(std::io::Error),
    ParseXmlError(xml::Error),
}

#[derive(Debug)]
pub(crate) struct ErrorResponse(String);

pub(crate) trait Client {
    fn request(&self, url: Url, auth: Option<(&str, &str)>) -> Result<Box<dyn Read>, ClientError>;
}

#[derive(Debug)]
pub(crate) enum ClientError {
    CoordinatesNotFound(Url),
    ClientError(String),
    ServerError(String),
    ErrorWhileReadingError(std::io::Error),
}

pub(crate) struct UrlResolver {
    server: String,
    server_url: Url,
    auth: Option<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct InvalidResolver {
    server: String,
    error: String,
}

impl UrlResolver {
    pub(crate) fn new(
        server: String,
        auth: Option<(String, String)>,
    ) -> Result<Self, InvalidResolver> {
        let url = match Url::parse(server.as_str()) {
            Ok(url) => url,
            Err(e) => {
                return Err(InvalidResolver {
                    server,
                    error: e.to_string(),
                })
            }
        };
        if url.cannot_be_a_base() {
            return Err(InvalidResolver {
                server,
                error: format!("Cannot be a base"),
            });
        }
        Ok(Self {
            server,
            server_url: url,
            auth,
        })
    }

    fn url(&self, coordinates: &Coordinates) -> Url {
        let mut url = self.server_url.clone();

        url.path_segments_mut()
            .unwrap() // we did check during construction
            .extend(coordinates.group_id.split('.'))
            .push(&coordinates.artifact)
            .push("maven-metadata.xml");

        url
    }
}

impl Resolver for UrlResolver {
    fn resolve<T: Client>(&self, coordinates: &Coordinates, client: &T) -> Result<Versions, Error> {
        let url = self.url(coordinates);

        let auth = self.auth.as_ref().map(|a| (a.0.as_str(), a.1.as_str()));

        let response = match client.request(url, auth) {
            Ok(response) => response,
            Err(ce) => {
                let err = match ce {
                    ClientError::CoordinatesNotFound(url) => Error::CoordinatesNotFound {
                        coordinates: coordinates.clone(),
                        server: self.server.clone(),
                        url,
                    },
                    ClientError::ClientError(err) => {
                        Error::ClientError(self.server.clone(), ErrorResponse(err))
                    }
                    ClientError::ServerError(err) => {
                        Error::ServerError(self.server.clone(), ErrorResponse(err))
                    }
                    ClientError::ErrorWhileReadingError(err) => Error::ErrorWhileReadingError(err),
                };
                return Err(err);
            }
        };
        let meta_data: MetaData = xml::from_reader(response)?;
        let versions = meta_data.versioning.versions;
        Ok(versions)
    }
}
pub(crate) struct UreqClient {
    timeout: Duration,
}

impl UreqClient {
    pub(crate) fn with_default_timeout() -> Self {
        Self::new(Duration::from_secs(30))
    }

    pub(crate) fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Client for UreqClient {
    fn request(&self, url: Url, auth: Option<(&str, &str)>) -> Result<Box<dyn Read>, ClientError> {
        let mut request = ureq::get(url.as_str());
        if let Some((user, pass)) = auth {
            request.auth(user, pass);
        }

        let response = request.timeout(self.timeout).call();
        if response.status() == 404 {
            return Err(ClientError::CoordinatesNotFound(url));
        }
        if response.error() {
            let client_error = response.client_error();
            let body = response.into_string()?;

            let err = if client_error {
                ClientError::ClientError(body)
            } else {
                ClientError::ServerError(body)
            };
            return Err(err);
        }

        Ok(Box::new(response.into_reader()))
    }
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

impl From<std::io::Error> for ClientError {
    fn from(source: std::io::Error) -> Self {
        ClientError::ErrorWhileReadingError(source)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CoordinatesNotFound { coordinates, server, url } => write!(
                f,
                "The coordinates {}:{} could not be found using the resolver {}. This could be because the coordinates do not exist or because the server does not follow maven style publication. The following URL was tried and resulted in a 404: {}",
                style(&coordinates.group_id).red().bold(),
                style(&coordinates.artifact).red().bold(),
                style(server).cyan(),
                style(url).cyan().bold()
            ),
            Error::ClientError(url, _) => write!(
                f,
                "Could not read Maven metadata using the resolver {}. There is likely something wrong with your request, please check your inputs.",
                style(url).cyan()
            ),
            Error::ServerError(url, _) => write!(
                f,
                "Could not read Maven metadata using the resolver {}. There is likely something wrong with Maven central. Please try again later.",
                style(url).cyan()
            ),
            Error::ErrorWhileReadingError(_) => {
                write!(f, "Could not read the error response from Maven central. Maybe your internet connection is gone. Maven central could also be down.")
            }
            Error::ParseXmlError(_) => write!(f, "Unable to parse Maven metadata XML file. The resolver might not conform to the proper maven metadate format."),
        }
    }
}

impl Display for InvalidResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The resolver {} is an invalid URL. {}",
            style(self.server.as_str()).red().bold(),
            self.error
        )
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

impl std::error::Error for InvalidResolver {}
impl std::error::Error for ErrorResponse {}

#[derive(Debug, Deserialize)]
struct MetaData {
    versioning: Versioning,
}

#[derive(Debug, Deserialize)]
struct Versioning {
    versions: Versions,
}
