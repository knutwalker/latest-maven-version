use crate::{metadata::Parser, Coordinates, Versions};
use console::style;
use std::{fmt::Display, time::Duration};
use ureq::{Request, Response};
use url::Url;

pub(crate) trait Resolver {
    fn resolve<T: Client>(&self, coordinates: &Coordinates, client: &T) -> Result<Versions, Error>;
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

    fn into_err<T>(self, resolver: Url, url: Url, status: u16) -> Result<T, Error> {
        Err(self.err(resolver, url, status))
    }
}

#[derive(Debug)]
pub(crate) struct ErrorResponse(String);

pub(crate) trait Client {
    fn request(&self, request: Request) -> Response;
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

impl Resolver for UrlResolver {
    fn resolve<T: Client>(&self, coordinates: &Coordinates, client: &T) -> Result<Versions, Error> {
        let url = self.url(coordinates);
        let mut request = ureq::get(url.as_str());
        if let Some((user, pass)) = &self.auth {
            request.auth(user, pass);
        }

        let response = client.request(request);
        let status = response.status();

        if response.synthetic() {
            let error = response.synthetic_error();
            let error = error as *const Option<ureq::Error>;
            let error = error as *mut Option<ureq::Error>;
            //   == SAFETY ==
            // We call `take` on the result which replaces the error value with None before
            // the request is being dropped. The error is no longer owned by the request
            // and so will not result in a dangling pointer. We need write access to the request
            // field but the API only offers a shared reference.
            // We're also not doing anything with response anymore. We would use something
            // like into_synthetic_error or just clone the error, but neither option exists.
            // The only thing the response does is being dropped.
            // See also: https://github.com/algesten/ureq/issues/126
            let error = unsafe { &mut *error };
            let error = error.take().unwrap();
            return ErrorKind::RequestError(error).into_err(self.server.clone(), url, status);
        }

        if status == 404 {
            return ErrorKind::CoordinatesNotFound(coordinates.clone()).into_err(
                self.server.clone(),
                url,
                status,
            );
        }

        let is_error = response.error();
        let client_error = response.client_error();

        let body = response.into_string().map_err(|src| {
            ErrorKind::ReadBodyError(src).err(self.server.clone(), url.clone(), status)
        })?;

        // TODO: auth errors
        if is_error {
            let error = if client_error {
                ErrorKind::ClientError(body)
            } else {
                ErrorKind::ServerError(body)
            };
            return error.into_err(self.server.clone(), url, status);
        }

        let versions = Parser::parse_into(&body)
            .map_err(|src| ErrorKind::ParseBodyError(src).err(self.server.clone(), url, status))?;
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
    fn request(&self, mut request: Request) -> Response {
        request.timeout(self.timeout).call()
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
    use std::cell::RefCell;
    use test_case::test_case;

    struct FakeClient<'a> {
        error: RefCell<Option<ErrorKind>>,
        versions: &'a [&'static str],
    }

    impl From<ErrorKind> for FakeClient<'_> {
        fn from(e: ErrorKind) -> Self {
            Self {
                error: RefCell::new(Some(e)),
                versions: &[],
            }
        }
    }

    impl<'a> From<&'a [&'static str]> for FakeClient<'a> {
        fn from(versions: &'a [&'static str]) -> Self {
            Self {
                error: RefCell::new(None),
                versions,
            }
        }
    }

    impl<'a> Client for FakeClient<'a> {
        fn request(&self, _request: Request) -> Response {
            let mut error = self.error.borrow_mut();
            if let Some(error) = error.take() {
                match error {
                    ErrorKind::CoordinatesNotFound(_) => Response::new(404, "Not Found", ""),
                    ErrorKind::ClientError(e) => Response::new(400, "Bad Request", &e),
                    ErrorKind::ServerError(e) => Response::new(500, "Internal server error", &e),
                    ErrorKind::RequestError(e) => e.into(),
                    ErrorKind::ReadBodyError(_) | ErrorKind::ParseBodyError(_) => {
                        Response::new(500, "Internal server error", "")
                    }
                }
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

                Response::new(200, "OK", &response)
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

    #[test]
    fn test_url_resolver_resolve() {
        let resolver = UrlResolver::new("http://example.com", None).unwrap();
        let versions = vec!["1.0.0", "1.3.37", "1.33.7"];
        let versions = &versions[..];
        let client = FakeClient::from(versions);
        let actual = resolver
            .resolve(&Coordinates::new("com.foo", "bar.baz"), &client)
            .unwrap();

        assert_eq!(actual, Versions::from(versions));
    }

    #[test]
    fn test_url_resolver_failing() {
        let coordinates = Coordinates::new("foo", "bar");
        let server = Url::parse("http://example.com").unwrap();

        let resolver = UrlResolver::new(server.to_string(), None).unwrap();

        let client = FakeClient::from(ErrorKind::CoordinatesNotFound(coordinates.clone()));
        let actual = resolver.resolve(&coordinates, &client).unwrap_err();

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
