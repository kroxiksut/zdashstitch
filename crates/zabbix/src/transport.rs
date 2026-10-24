// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{Result, ZabbixError};
use futures_util::StreamExt;
use reqwest::{Client, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use url::Url;

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZabbixEndpoint(Url);

impl ZabbixEndpoint {
    pub fn parse(value: &str) -> Result<Self> {
        let url =
            Url::parse(value).map_err(|error| ZabbixError::InvalidEndpoint(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ZabbixError::InvalidEndpoint(
                "only http and https are allowed".to_owned(),
            ));
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(ZabbixError::InvalidEndpoint(
                "credentials and fragments are forbidden".to_owned(),
            ));
        }
        Ok(Self(url))
    }

    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

pub(crate) struct Transport {
    endpoint: ZabbixEndpoint,
    client: Client,
    next_id: AtomicU64,
}

impl Transport {
    pub(crate) fn new(endpoint: ZabbixEndpoint) -> Result<Self> {
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(ZabbixError::Client)?;
        Ok(Self {
            endpoint,
            client,
            next_id: AtomicU64::new(1),
        })
    }

    pub(crate) async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
        token: Option<&SecretString>,
    ) -> Result<T> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };
        let mut builder = self
            .client
            .post(self.endpoint.as_url().clone())
            .json(&request);
        if let Some(token) = token {
            builder = builder.bearer_auth(token.expose_secret());
        }
        let response = builder.send().await.map_err(ZabbixError::Transport)?;
        if !response.status().is_success() {
            return Err(ZabbixError::HttpStatus(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ZabbixError::ResponseTooLarge {
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(ZabbixError::Transport)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(ZabbixError::ResponseTooLarge {
                    limit: MAX_RESPONSE_BYTES,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: RpcResponse<T> =
            serde_json::from_slice(&bytes).map_err(ZabbixError::InvalidResponse)?;
        if response.id != id {
            return Err(ZabbixError::ResponseIdMismatch);
        }
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(ZabbixError::Api {
                method: method.to_owned(),
                code: error.code,
                message: error.message,
                data: error.data,
            }),
            _ => Err(ZabbixError::Authentication(
                "response contains neither a unique result nor an error".to_owned(),
            )),
        }
    }

    pub(crate) fn endpoint_identity(&self) -> &str {
        self.endpoint.as_url().as_str()
    }
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
    id: u64,
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    id: u64,
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<String>,
}
