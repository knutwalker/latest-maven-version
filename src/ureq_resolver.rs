use super::{Client, Error as SuperError, ErrorKind, IntoError};
use crate::Coordinates;
use async_trait::async_trait;
use std::time::Duration;
use ureq::{Agent, AgentBuilder, Error};
use url::Url;

pub(super) struct UreqClient {
    agent: Agent,
}

impl UreqClient {
    pub(super) fn with_default_timeout() -> Self {
        Self::new(Duration::from_secs(30))
    }

    pub(super) fn new(timeout: Duration) -> Self {
        let agent = AgentBuilder::new().timeout(timeout).build();
        Self { agent }
    }
}

#[async_trait]
impl Client for UreqClient {
    type Err = Error;

    async fn request(
        &self,
        url: &Url,
        auth: Option<&(String, String)>,
        _coordinates: &Coordinates,
    ) -> Result<String, Self::Err> {
        let mut request = self.agent.request_url("GET", url);

        if let Some((user, pass)) = auth {
            let header = format!("{}:{}", user, pass);
            let header = base64::encode(header);
            let header = format!("Basic: {}", header);
            request = request.set("Authorization", &header);
        }

        request
            .call()
            .and_then(|response| response.into_string().map_err(|ioe| Error::from(ioe)))
    }
}

impl IntoError for Error {
    fn into_error(self, coordinates: &Coordinates, resolver: &Url, url: Url) -> SuperError {
        match self {
            Error::Transport(e) => {
                ErrorKind::TransportError(Box::new(e)).err(resolver.clone(), url)
            }
            Error::Status(404, _) => {
                ErrorKind::CoordinatesNotFound(coordinates.clone()).err(resolver.clone(), url)
            }
            Error::Status(status, response) => match response.into_string() {
                Ok(body) => {
                    let error = if status / 100 == 4 {
                        ErrorKind::ClientError(status, body)
                    } else {
                        ErrorKind::ServerError(status, body)
                    };
                    error.err(resolver.clone(), url)
                }
                Err(src) => ErrorKind::ReadBodyError(status, Box::new(src))
                    .err(resolver.clone(), url.clone()),
            },
        }
    }
}
