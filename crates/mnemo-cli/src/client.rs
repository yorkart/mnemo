use anyhow::{Context, bail};
use serde_json::Value;

pub struct HttpClient {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new(base_url: String, token: Option<String>) -> anyhow::Result<Self> {
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client: reqwest::Client::builder().no_proxy().build()?,
        })
    }

    pub async fn get(&self, path: &str) -> anyhow::Result<Value> {
        self.send(self.client.get(format!("{}{}", self.base_url, path)), None)
            .await
    }

    pub async fn post<T: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
        idempotency_key: Option<&str>,
    ) -> anyhow::Result<Value> {
        self.send(
            self.client
                .post(format!("{}{}", self.base_url, path))
                .json(body),
            idempotency_key,
        )
        .await
    }

    pub async fn put<T: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
    ) -> anyhow::Result<Value> {
        self.send(
            self.client
                .put(format!("{}{}", self.base_url, path))
                .json(body),
            None,
        )
        .await
    }

    pub async fn patch<T: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
    ) -> anyhow::Result<Value> {
        self.send(
            self.client
                .patch(format!("{}{}", self.base_url, path))
                .json(body),
            None,
        )
        .await
    }

    async fn send(
        &self,
        mut request: reqwest::RequestBuilder,
        idempotency_key: Option<&str>,
    ) -> anyhow::Result<Value> {
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(idempotency_key) = idempotency_key {
            request = request.header("Idempotency-Key", idempotency_key);
        }
        let response = request.send().await.context("request failed")?;
        let status = response.status();
        let text = response.text().await.context("read response body")?;
        let value =
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| serde_json::json!({ "body": text }));
        if !status.is_success() {
            bail!("mnemo API returned {status}: {value}");
        }
        Ok(value)
    }
}
