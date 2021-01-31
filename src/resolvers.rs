use crate::{metadata::Parser, Coordinates, Versions};
use async_trait::async_trait;
use console::style;
use std::fmt::Display;
use url::Url;

#[path = "reqwest_resolver.rs"]
mod reqwest_resolver;

pub(crate) fn client() -> impl Client {
    reqwest_resolver::ReqwestClient::with_default_timeout()
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
    error: ErrorKind,
}

#[derive(Debug)]
pub(crate) enum ErrorKind {
    /// Could not send the request because it was not valid
    InvalidRequest(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// Could not connect to the server
    ServerNotFound, // (Box<dyn std::error::Error + Send + Sync + 'static>),
    /// Could not read from the server within the given timeout
    ServerNotAvailable, // (Box<dyn std::error::Error + Send + Sync + 'static>),
    /// Could not read from the serveer for some reason
    TransportError(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// Response caught in redirect loop
    TooManyRedirects, // (Box<dyn std::error::Error + Send + Sync + 'static>),
    /// Could not find the coordinates on the server
    CoordinatesNotFound(Coordinates),
    /// Could not read the response body from the server
    ReadBodyError(u16, Box<dyn std::error::Error + Send + Sync + 'static>),
    /// Any 4xx response
    ClientError(u16, String),
    /// Any 5xx response
    ServerError(u16, String),
    /// Could not parse the xml response
    ParseBodyError(xmlparser::Error),
}

impl ErrorKind {
    fn err(self, resolver: Url, url: Url) -> Error {
        Error {
            resolver,
            url,
            error: self,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ErrorResponse(String);

#[async_trait]
pub(crate) trait Client: Send + Sync {
    async fn request(
        &self,
        url: &Url,
        auth: Option<&(String, String)>,
        coordinates: &Coordinates,
    ) -> Result<String, ErrorKind>;
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

        let response = client.request(&url, self.auth.as_ref(), coordinates).await;
        let body = match response {
            Ok(body) => body,
            Err(err) => return Err(err.err(self.server.clone(), url)),
        };

        let versions = Parser::parse_into(&body)
            .map_err(|src| ErrorKind::ParseBodyError(src).err(self.server.clone(), url))?;
        Ok(versions)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Error {
            resolver,
            url,
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
            ErrorKind::ClientError(sc, error) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThere is likely something wrong with your request, please check your inputs.\nThe URL '{}' was tried and resulted in a {} with the body\n\n{}",
                style(resolver).cyan(),
                style(url).cyan().bold(),
                style(*sc).yellow().bold(),
                error
            ),
            ErrorKind::ServerError(sc, error) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThere is likely something wrong with Maven central.\nThe URL '{}' was tried and resulted in a {} with the body\n\n{}\n\nIt's probably best to try later.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
                style(*sc).red().bold(),
                error
            ),
            ErrorKind::ReadBodyError(sc, _) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThe response could not be read or was not valid UTF-8.\nMaybe your internet connection is gone?\nMaven central could also be down.\nThe URL '{}' was tried and resulted in a {}.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
                style(*sc).red().bold(),
            ),
            ErrorKind::InvalidRequest(_) => write!(
                f,
                "Could not send the request to the resolver.\nThere is probably something wrong the resolver '{}' or the tried URL '{}'.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
            ),
            ErrorKind::ServerNotFound => write!(
                f,
                "Could not connect to the resolver {}.\nMaybe your internet is gone? The resolver could also be down.\nThe URL '{}' was tried.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
            ),
            ErrorKind::ServerNotAvailable => write!(
                f,
                "Did not get a response from the resolver {}.\nMaybe your internet is gone or very slow? The resolver could also be down or under load.\nThe URL '{}' was tried.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
            ),
            ErrorKind::TransportError(_) => write!(
                f,
                "Could not read Maven metadata using the resolver {}.\nThere is likely something wrong with your request, please check your inputs.\nThe URL '{}' was tried.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
            ),
            ErrorKind::TooManyRedirects => write!(
                f,
                "The resolver {} reponded with a redirect loop.\nThere is likely something wrong with your request, please check your inputs.\nThe URL '{}' was tried.",
                style(resolver).cyan(),
                style(url).cyan().bold(),
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
            ErrorKind::InvalidRequest(src) => Some(&**src),
            ErrorKind::TransportError(src) => Some(&**src),
            ErrorKind::ReadBodyError(_, src) => Some(&**src),
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
        async fn request(
            &self,
            _url: &Url,
            _auth: Option<&(String, String)>,
            _coordinates: &Coordinates,
        ) -> Result<String, ErrorKind> {
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

                Ok(response)
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
