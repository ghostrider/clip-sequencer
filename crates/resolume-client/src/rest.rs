//! REST client (notes §2–§4, §8). All calls are async and belong in worker tasks, never on the
//! engine's timing path.

use std::time::Duration;

use seq_core::index::{ClipInfo, ClipState};

use crate::composition::{parse_clip, parse_composition, CompositionSnapshot, ParseError};

#[derive(Debug, thiserror::Error)]
pub enum RestError {
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{method} {path} returned {status}: {body}")]
    Status {
        method: &'static str,
        path: String,
        status: u16,
        body: String,
    },
    #[error(transparent)]
    Parse(#[from] ParseError),
}

/// A fetched thumbnail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailData {
    pub png: Vec<u8>,
    /// `ETag` header; equals the clip's `thumbnail.last_update`.
    pub etag: Option<String>,
}

#[derive(Clone)]
pub struct RestClient {
    base: String,
    http: reqwest::Client,
}

impl RestClient {
    pub fn new(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_millis(500))
            // No keep-alive: Resolume waits on quit until all its client connections are gone,
            // so an idle pooled connection would block it (notes §12). Requests are rare.
            .pool_max_idle_per_host(0)
            .build()
            .expect("HTTP client");
        Self {
            base: base.into(),
            http,
        }
    }

    async fn checked(
        &self,
        method: &'static str,
        path: &str,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, RestError> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(RestError::Status {
                method,
                path: path.to_owned(),
                status: resp.status().as_u16(),
                body: resp.text().await.unwrap_or_default(),
            })
        }
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response, RestError> {
        let resp = self.http.get(format!("{}{path}", self.base)).send().await?;
        self.checked("GET", path, resp).await
    }

    async fn post(&self, path: &str, body: Option<String>) -> Result<(), RestError> {
        let mut req = self.http.post(format!("{}{path}", self.base));
        if let Some(b) = body {
            req = req
                .header(reqwest::header::CONTENT_TYPE, "text/plain")
                .body(b);
        }
        let resp = req.send().await?;
        self.checked("POST", path, resp).await.map(|_| ())
    }

    pub async fn product(&self) -> Result<serde_json::Value, RestError> {
        Ok(self.get("/product").await?.json().await?)
    }

    /// Raw composition JSON. Not valid during a deck switch (notes §4).
    pub async fn composition_text(&self) -> Result<String, RestError> {
        Ok(self.get("/composition").await?.text().await?)
    }

    pub async fn composition(&self) -> Result<CompositionSnapshot, RestError> {
        Ok(parse_composition(&self.composition_text().await?)?)
    }

    /// One slot by position, `GET /composition/layers/{L}/clips/{C}` (1-based, notes §15). Used
    /// to read a slot lane's slot just before its step (SPEC §4.5): drags and drops in Resolume's
    /// UI are often not pushed. Not valid during a deck switch (notes §4).
    pub async fn clip_at(
        &self,
        layer: usize,
        column: usize,
    ) -> Result<(ClipInfo, Option<ClipState>), RestError> {
        let text = self
            .get(&format!("/composition/layers/{layer}/clips/{column}"))
            .await?
            .text()
            .await?;
        Ok(parse_clip(&text, layer, column)?)
    }

    pub async fn thumbnail(&self, clip_id: u64) -> Result<ThumbnailData, RestError> {
        let resp = self
            .get(&format!("/composition/clips/by-id/{clip_id}/thumbnail"))
            .await?;
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_matches('"').to_owned());
        Ok(ThumbnailData {
            png: resp.bytes().await?.to_vec(),
            etag,
        })
    }

    /// `POST /composition/decks/{n}/select` (1-based). Returns before the switch completes.
    pub async fn select_deck(&self, deck: usize) -> Result<(), RestError> {
        self.post(&format!("/composition/decks/{deck}/select"), None)
            .await
    }

    /// Sets a custom thumbnail (multipart, field `file`); the clip's source is unchanged.
    pub async fn set_clip_thumbnail(&self, clip_id: u64, png: Vec<u8>) -> Result<(), RestError> {
        let path = format!("/composition/clips/by-id/{clip_id}/thumbnail");
        let part = reqwest::multipart::Part::bytes(png)
            .file_name("thumbnail.png")
            .mime_str("image/png")?;
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .multipart(reqwest::multipart::Form::new().part("file", part))
            .send()
            .await?;
        self.checked("POST", &path, resp).await.map(|_| ())
    }

    /// Loads a file (`file:///…`, percent-encoded) or source (`source:///video/<name>`) into a clip.
    pub async fn open_clip(&self, clip_id: u64, url: &str) -> Result<(), RestError> {
        self.post(
            &format!("/composition/clips/by-id/{clip_id}/open"),
            Some(url.to_owned()),
        )
        .await
    }

    /// `POST /composition/clips/by-id/{id}/clear`: the slot becomes `Empty` and keeps its id
    /// (notes §2.1, §8).
    pub async fn clear_clip(&self, clip_id: u64) -> Result<(), RestError> {
        self.post(&format!("/composition/clips/by-id/{clip_id}/clear"), None)
            .await
    }

    /// `POST /composition/layers/add` without a body: appends a layer on top (highest index,
    /// named `"Layer #"`, one empty slot per column); existing ids keep their place (notes §16).
    pub async fn add_layer(&self) -> Result<(), RestError> {
        self.post("/composition/layers/add", None).await
    }

    /// `POST /composition/columns/add` without a body: appends a column at the end; existing
    /// clips keep their positions (notes §16).
    pub async fn add_column(&self) -> Result<(), RestError> {
        self.post("/composition/columns/add", None).await
    }

    /// One layer's raw JSON, `GET /composition/layers/{L}` (1-based, notes §15).
    pub async fn layer_json(&self, layer: usize) -> Result<serde_json::Value, RestError> {
        Ok(self
            .get(&format!("/composition/layers/{layer}"))
            .await?
            .json()
            .await?)
    }

    /// Names of the video sources (generators and FFGL source plugins), `GET /sources`
    /// (notes §7, §16).
    pub async fn video_sources(&self) -> Result<Vec<String>, RestError> {
        let v: serde_json::Value = self.get("/sources").await?.json().await?;
        Ok(v["video"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| s["name"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }
}
