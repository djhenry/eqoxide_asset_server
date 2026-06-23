use crate::cas::Cas;
use crate::manifest::Manifest;

pub struct SyncStats {
    pub chunks_total: usize,
    pub chunks_downloaded: usize,
    pub bytes_downloaded: u64,
}

pub struct SyncClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl SyncClient {
    pub async fn login(base: &str, username: &str, password: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::new();
        let resp = http
            .post(format!("{base}/auth"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send().await?
            .error_for_status()?;
        let token = resp.json::<serde_json::Value>().await?["token"]
            .as_str().ok_or_else(|| anyhow::anyhow!("no token in response"))?
            .to_string();
        Ok(Self { base: base.to_string(), token, http })
    }

    pub async fn sync_set(&self, set: &str, local: &Cas) -> anyhow::Result<SyncStats> {
        let manifest: Manifest = self
            .http
            .get(format!("{}/manifest/{set}", self.base))
            .bearer_auth(&self.token)
            .send().await?
            .error_for_status()?
            .json().await?;

        // unique ordered chunk hashes across all files
        let mut wanted: Vec<String> = Vec::new();
        for f in &manifest.files {
            for h in &f.chunks {
                if !wanted.contains(h) {
                    wanted.push(h.clone());
                }
            }
        }

        let mut stats = SyncStats { chunks_total: wanted.len(), chunks_downloaded: 0, bytes_downloaded: 0 };
        for hash in wanted {
            if local.has(&hash) {
                continue;
            }
            let bytes = self
                .http
                .get(format!("{}/chunk/{hash}", self.base))
                .bearer_auth(&self.token)
                .send().await?
                .error_for_status()?
                .bytes().await?;
            local.put(&bytes)?;
            stats.chunks_downloaded += 1;
            stats.bytes_downloaded += bytes.len() as u64;
        }
        Ok(stats)
    }
}
