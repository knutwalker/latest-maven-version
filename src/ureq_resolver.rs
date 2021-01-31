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
    ) -> Result<(u16, String), Self::Err> {
        let mut request = self.agent.request_url("GET", url);

        if let Some((user, pass)) = auth {
            let header = format!("{}:{}", user, pass);
            let header = base64::encode(header);
            let header = format!("Basic: {}", header);
            request = request.set("Authorization", &header);
        }

        request.call().and_then(|response| {
            let status = response.status();
            let body = response.into_string().map_err(|ioe| Error::from(ioe))?;
            Ok((status, body))
        })
    }
}

impl IntoError for Error {
    fn into_error(self, coordinates: &Coordinates, server: &Url, url: Url) -> SuperError {
        match self {
            Error::Transport(e) => {
                ErrorKind::RequestError(Error::Transport(e)).err(server.clone(), url, 400)
            }
            Error::Status(404, _) => {
                ErrorKind::CoordinatesNotFound(coordinates.clone()).err(server.clone(), url, 404)
            }
            Error::Status(status, response) => match response.into_string() {
                Ok(body) => {
                    let error = if status / 100 == 4 {
                        ErrorKind::ClientError(body)
                    } else {
                        ErrorKind::ServerError(body)
                    };
                    error.err(server.clone(), url, status)
                }
                Err(src) => ErrorKind::ReadBodyError(src).err(server.clone(), url.clone(), status),
            },
        }
    }
}
