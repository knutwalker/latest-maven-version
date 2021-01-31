use crate::{metadata::Parser, Coordinates, Versions};
use async_trait::async_trait;
use console::style;
use std::fmt::Display;
use url::Url;

#[path = "ureq_resolver.rs"]
mod ureq_resolver;

pub(crate) fn client() -> impl Client {
    ureq_resolver::UreqClient::with_default_timeout()
}

#[async_trait]
pub(crate) trait Resolver {
    async fn resolve<T: Client>(
        &self,
        coordinates: &Coordinates,
        client: &T,
    ) -> Result<Versions, Error>;
}

#[derive(Debug)]
pub(crate) struct Error {
    resolver: Url,
    url: Url,
    status: u16,
    error: ErrorKind,
}

#[derive(Debug)]
pub(crate) enum ErrorKind {
    CoordinatesNotFound(Coordinates),
    ClientError(String),
    ServerError(String),
    RequestError(ureq::Error),
    ReadBodyError(std::io::Error),
    ParseBodyError(xmlparser::Error),
}

impl ErrorKind {
    fn err(self, resolver: Url, url: Url, status: u16) -> Error {
        Error {
            resolver,
            url,
            status,
            error: self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ErrorResponse(String);

#[async_trait]
pub(crate) trait Client: Send + Sync {
    type Err: IntoError + std::fmt::Debug;

    async fn request(
        &self,
        url: &Url,
        auth: Option<&(String, String)>,
    ) -> Result<(u16, String), Self::Err>;
}

pub(crate) trait IntoError {
    fn into_error(self, coordinates: &Coordinates, server: &Url, url: Url) -> Error;
}

impl IntoError for ErrorKind {
    fn into_error(self, _coordinates: &Coordinates, server: &Url, url: Url) -> Error {
        let status = match &self {
            ErrorKind::CoordinatesNotFound(_) => 404,
            ErrorKind::ClientError(_) => 400,
            ErrorKind::ServerError(_) => 500,
            ErrorKind::RequestError(ureq_error) => match ureq_error {
                ureq::Error::Status(status, _) => *status,
                ureq::Error::Transport(_) => 500,
            },
            ErrorKind::ReadBodyError(_) => 500,
            ErrorKind::ParseBodyError(_) => 500,
        };
        self.err(server.clone(), url, status)
    }
}
#[derive(Debug)]
pub(crate) struct UrlResolver {
    server: Url,
    auth: Option<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct InvalidResolver {
    server: String,
    error: String,
}

impl UrlResolver {
    pub(crate) fn new<T>(server: T, auth: Option<(String, String)>) -> Result<Self, InvalidResolver>
    where
        T: Into<String> + AsRef<str>,
    {
        let server = match Url::parse(server.as_ref()) {
            Ok(url) => url,
            Err(e) => {
                return Err(InvalidResolver {
                    server: server.into(),
                    error: e.to_string(),
                })
            }
        };
        if server.cannot_be_a_base() {
            return Err(InvalidResolver {
                server: server.to_string(),
                error: String::from("Cannot be a base"),
            });
        }
        Ok(Self { server, auth })
    }

    fn url(&self, coordinates: &Coordinates) -> Url {
        let mut url = self.server.clone();

        url.path_segments_mut()
            .unwrap() // we did check during construction
            .extend(coordinates.group_id.split('.'))
            .push(&coordinates.artifact)
            .push("maven-metadata.xml");

        url
    }
}

#[async_trait]
impl Resolver for UrlResolver {
    async fn resolve<T: Client>(
        &self,
        coordinates: &Coordinates,
        client: &T,
    ) -> Result<Versions, Error> {
        let url = self.url(coordinates);

        let response = client.request(&url, self.auth.as_ref()).await;
        let (status, body) = match response {
            Ok(body) => body,
            Err(err) => {
                return Err(err.into_error(coordinates, &self.server, url));
            }
        };

        let versions = Parser::parse_into(&body)
            .map_err(|src| ErrorKind::ParseBodyError(src).err(self.server.clone(), url, status))?;
        Ok(versions)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Error {
            resolver,
            url,
            status,
            error,
        } = self;
        match error {
            ErrorKind::CoordinatesNotFound(coordinates) => write!(
                f,
                "The coordinates {}:{} could not be found using the resolver {}.\nThis could be because the coordinates do not exist or because the server does not follow maven style publication.\nThe following URL was tried and resulted in a 404: {}",
                style(&coordinates.group_id).red().bold(),
                style(&coordinates.artifact).red().bold(),
                style(resolver).cyan(),
                style(url).cyan().bold()
            ),
            ErrorKind::ClientError(error) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThere is likely something wrong with your request, please check your inputs.\nThe URL '{}' was tried and resulted in a {} with the body\n\n{}",
                style(resolver).cyan(),
                style(url).cyan().bold(),
                style(*status).yellow().bold(),
                error
            ),
            ErrorKind::ServerError(error) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThere is likely something wrong with Maven central.\nThe URL '{}' was tried and resulted in a {} with the body\n\n{}\n\nIt's probably best to try later.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
                style(*status).red().bold(),
                error
            ),
            ErrorKind::RequestError(_) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThere is likely something wrong with your request, please check your inputs.",
                style(resolver).cyan(),
            ),
            ErrorKind::ReadBodyError(_) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThe response could not be read or was not valid UTF-8.\nMaybe your internet connection is gone?\nMaven central could also be down.\nThe URL '{}' was tried and resulted in a {}.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
                style(*status).red().bold(),
            ),
            ErrorKind::ParseBodyError(_) => write!(
                f,
                "Unable to parse Maven metadata XML file.\nThe resolver {} might not conform to the proper maven metadata format.\nThe URL '{}' was tried.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
            ),
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
        match &self.error {
            ErrorKind::RequestError(src) => Some(src),
            ErrorKind::ReadBodyError(src) => Some(src),
            ErrorKind::ParseBodyError(src) => Some(src),
            _ => None,
        }
    }
}

impl std::error::Error for InvalidResolver {}
impl std::error::Error for ErrorResponse {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use test_case::test_case;

    struct FakeClient<'a> {
        error: Arc<Mutex<Option<ErrorKind>>>,
        versions: &'a [&'static str],
    }

    impl From<ErrorKind> for FakeClient<'_> {
        fn from(e: ErrorKind) -> Self {
            Self {
                error: Arc::new(Mutex::new(Some(e))),
                versions: &[],
            }
        }
    }

    impl<'a> From<&'a [&'static str]> for FakeClient<'a> {
        fn from(versions: &'a [&'static str]) -> Self {
            Self {
                error: Arc::new(Mutex::new(None)),
                versions,
            }
        }
    }

    #[async_trait]
    impl<'a> Client for FakeClient<'a> {
        type Err = ErrorKind;

        async fn request(
            &self,
            _url: &Url,
            _auth: Option<&(String, String)>,
        ) -> Result<(u16, String), Self::Err> {
            let mut error = self.error.lock().unwrap();
            if let Some(error) = error.take() {
                Err(error)
            } else {
                let versions = self
                    .versions
                    .iter()
                    .map(|v| format!("<version>{}</version>", v))
                    .collect::<String>();

                let response = format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <metadata>
                      <versioning>
                        <versions>
                          {}
                        </versions>
                      </versioning>
                    </metadata>
                    "#,
                    versions
                );

                Ok((200, response))
            }
        }
    }

    #[test]
    fn test_url_resolver_url() {
        let resolver = UrlResolver::new("http://example.com", None).unwrap();
        let url = resolver.url(&Coordinates::new("com.foo", "bar.baz"));
        assert_eq!(
            url,
            Url::parse("http://example.com/com/foo/bar.baz/maven-metadata.xml").unwrap()
        )
    }

    #[tokio::test]
    async fn test_url_resolver_resolve() {
        let resolver = UrlResolver::new("http://example.com", None).unwrap();
        let versions = vec!["1.0.0", "1.3.37", "1.33.7"];
        let versions = &versions[..];
        let client = FakeClient::from(versions);
        let actual = resolver
            .resolve(&Coordinates::new("com.foo", "bar.baz"), &client)
            .await
            .unwrap();

        assert_eq!(actual, Versions::from(versions));
    }

    #[tokio::test]
    async fn test_url_resolver_failing() {
        let coordinates = Coordinates::new("foo", "bar");
        let server = Url::parse("http://example.com").unwrap();

        let resolver = UrlResolver::new(server.to_string(), None).unwrap();

        let client = FakeClient::from(ErrorKind::CoordinatesNotFound(coordinates.clone()));
        let actual = resolver.resolve(&coordinates, &client).await.unwrap_err();

        let Error {
            resolver: actual_server,
            url,
            status: _,
            error,
        } = actual;
        if let ErrorKind::CoordinatesNotFound(actual_coordinates) = error {
            assert_eq!(actual_coordinates, coordinates);
            assert_eq!(actual_server, server);
            assert_eq!(url, resolver.url(&coordinates));
        } else {
            panic!("Expected CoordinatesNotFound")
        }
    }

    #[test_case("http:/foo bar" => "invalid domain character")]
    #[test_case("foobar" => "relative URL without a base")]
    #[test_case("data:text/plain,foobar" => "Cannot be a base")]
    fn test_url_resolver_invalid_url(url: &str) -> String {
        UrlResolver::new(url, None).unwrap_err().error
    }
}
