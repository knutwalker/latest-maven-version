use super::{Client as CrateClient, ErrorKind};
use crate::Coordinates;
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use std::time::Duration;
use url::Url;

// Name your user agent after your app?
static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

pub(super) struct ReqwestClient {
    client: Client,
}

impl ReqwestClient {
    pub(super) fn with_default_timeout() -> Self {
        Self::new(Duration::from_secs(30))
    }

    pub(super) fn new(timeout: Duration) -> Self {
        let client = Client::builder()
            .user_agent(APP_USER_AGENT)
            .gzip(true)
            .timeout(timeout)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .use_rustls_tls()
            .build()
            .unwrap();
        Self { client }
    }
}

#[async_trait]
impl CrateClient for ReqwestClient {
    type Err = ErrorKind;

    async fn request(
        &self,
        url: &Url,
        auth: Option<&(String, String)>,
        coordinates: &Coordinates,
    ) -> Result<String, Self::Err> {
        let mut request = self.client.get(url.clone());

        if let Some((user, pass)) = auth {
            request = request.basic_auth(user, Some(pass));
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                eprintln!("error = {0:#?}: {0}", error);
                return Err(if error.is_builder() {
                    ErrorKind::InvalidRequest(Box::new(error))
                } else if error.is_connect() {
                    ErrorKind::ServerNotFound
                } else if error.is_timeout() {
                    ErrorKind::ServerNotAvailable
                } else if error.is_redirect() {
                    ErrorKind::TooManyRedirects
                } else {
                    ErrorKind::TransportError(Box::new(error))
                });
            }
        };

        if response.status() == StatusCode::NOT_FOUND {
            return Err(ErrorKind::CoordinatesNotFound(coordinates.clone()));
        }

        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                eprintln!("error = {0:#?}: {0}", error);
                return Err(ErrorKind::ReadBodyError(status.as_u16(), Box::new(error)));
            }
        };

        if status.is_client_error() {
            return Err(ErrorKind::ClientError(status.as_u16(), body));
        }
        if status.is_server_error() {
            return Err(ErrorKind::ServerError(status.as_u16(), body));
        }

        Ok(body)
    }
}
