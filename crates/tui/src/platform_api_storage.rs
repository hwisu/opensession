use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;

pub struct PlatformApiStorage {
    client: reqwest::blocking::Client,
    token: String,
}

enum Platform {
    GitHub { owner: String, repo: String },
    GitLab { project_path: String },
}

impl PlatformApiStorage {
    pub fn new(token: String) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            token,
        }
    }

    /// Upload a session file to the `opensession` branch via platform API.
    /// Returns the raw content URL on success.
    pub fn store(
        &self,
        remote_url: &str,
        session_id: &str,
        jsonl: &[u8],
    ) -> Result<String, String> {
        let platform = detect_platform(remote_url)?;
        let rel_path = format!(".opensession/{session_id}.hail.jsonl");

        match &platform {
            Platform::GitHub { owner, repo } => {
                self.github_store(owner, repo, &rel_path, jsonl)?;
            }
            Platform::GitLab { project_path } => {
                self.gitlab_store(project_path, &rel_path, jsonl)?;
            }
        }

        Ok(opensession_git_native::generate_raw_url(
            remote_url, &rel_path,
        ))
    }

    // ── GitHub ───────────────────────────────────────────────────────────

    fn gh_get(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "opensession")
    }

    fn gh_post(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "opensession")
    }

    fn gh_put(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .put(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "opensession")
    }

    fn github_store(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        content: &[u8],
    ) -> Result<(), String> {
        let base = format!("https://api.github.com/repos/{owner}/{repo}");

        let branch_exists = self
            .gh_get(&format!("{base}/branches/opensession"))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        if !branch_exists {
            self.github_create_orphan(&base, path, content)?;
            return Ok(());
        }

        // Branch exists — check if file already exists (need sha for update)
        let file_sha = self.github_file_sha(&base, path);
        self.github_put_contents(&base, path, content, file_sha.as_deref())
    }

    fn github_create_orphan(&self, base: &str, path: &str, content: &[u8]) -> Result<(), String> {
        // 1. Create blob
        let blob: ShaResponse = self
            .gh_post(&format!("{base}/git/blobs"))
            .json(&serde_json::json!({
                "content": STANDARD.encode(content),
                "encoding": "base64"
            }))
            .send()
            .map_err(|e| format!("blob: {e}"))?
            .json()
            .map_err(|e| format!("blob parse: {e}"))?;

        // 2. Create tree
        let tree: ShaResponse = self
            .gh_post(&format!("{base}/git/trees"))
            .json(&serde_json::json!({
                "tree": [{
                    "path": path,
                    "mode": "100644",
                    "type": "blob",
                    "sha": blob.sha
                }]
            }))
            .send()
            .map_err(|e| format!("tree: {e}"))?
            .json()
            .map_err(|e| format!("tree parse: {e}"))?;

        // 3. Create commit (no parents → orphan)
        let commit: ShaResponse = self
            .gh_post(&format!("{base}/git/commits"))
            .json(&serde_json::json!({
                "message": "Add session",
                "tree": tree.sha,
                "parents": []
            }))
            .send()
            .map_err(|e| format!("commit: {e}"))?
            .json()
            .map_err(|e| format!("commit parse: {e}"))?;

        // 4. Create ref
        let resp = self
            .gh_post(&format!("{base}/git/refs"))
            .json(&serde_json::json!({
                "ref": "refs/heads/opensession",
                "sha": commit.sha
            }))
            .send()
            .map_err(|e| format!("ref: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("ref create failed: {body}"));
        }
        Ok(())
    }

    fn github_file_sha(&self, base: &str, path: &str) -> Option<String> {
        self.gh_get(&format!("{base}/contents/{path}?ref=opensession"))
            .send()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.json::<ShaResponse>().ok())
            .map(|f| f.sha)
    }

    fn github_put_contents(
        &self,
        base: &str,
        path: &str,
        content: &[u8],
        sha: Option<&str>,
    ) -> Result<(), String> {
        let mut body = serde_json::json!({
            "message": "Update session",
            "content": STANDARD.encode(content),
            "branch": "opensession"
        });
        if let Some(sha) = sha {
            body["sha"] = serde_json::Value::String(sha.to_string());
        }

        let resp = self
            .gh_put(&format!("{base}/contents/{path}"))
            .json(&body)
            .send()
            .map_err(|e| format!("upload: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(format!("GitHub upload failed ({status}): {body}"));
        }
        Ok(())
    }

    // ── GitLab ──────────────────────────────────────────────────────────

    fn gl_get(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.get(url).header("PRIVATE-TOKEN", &self.token)
    }

    fn gl_post(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.post(url).header("PRIVATE-TOKEN", &self.token)
    }

    fn gl_put(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        self.client.put(url).header("PRIVATE-TOKEN", &self.token)
    }

    fn gitlab_store(&self, project_path: &str, path: &str, content: &[u8]) -> Result<(), String> {
        // GitLab API requires URL-encoded project path
        let encoded_project = project_path.replace('/', "%2F");
        let base = format!("https://gitlab.com/api/v4/projects/{encoded_project}");

        let branch_exists = self
            .gl_get(&format!("{base}/repository/branches/opensession"))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        if !branch_exists {
            return self.gitlab_create_branch_with_file(&base, path, content);
        }

        // Branch exists — check if file exists
        let encoded_path = path.replace('/', "%2F");
        let file_exists = self
            .gl_get(&format!(
                "{base}/repository/files/{encoded_path}?ref=opensession"
            ))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        let file_body = serde_json::json!({
            "branch": "opensession",
            "commit_message": if file_exists { "Update session" } else { "Add session" },
            "content": STANDARD.encode(content),
            "encoding": "base64"
        });

        let url = format!("{base}/repository/files/{encoded_path}");
        let resp = if file_exists {
            self.gl_put(&url).json(&file_body).send()
        } else {
            self.gl_post(&url).json(&file_body).send()
        };

        let resp = resp.map_err(|e| format!("GitLab file op: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(format!("GitLab file op failed ({status}): {body}"));
        }
        Ok(())
    }

    fn gitlab_create_branch_with_file(
        &self,
        base: &str,
        path: &str,
        content: &[u8],
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "branch": "opensession",
            "start_branch": "main",
            "commit_message": "Add session",
            "actions": [{
                "action": "create",
                "file_path": path,
                "content": STANDARD.encode(content),
                "encoding": "base64"
            }]
        });

        let resp = self
            .gl_post(&format!("{base}/repository/commits"))
            .json(&body)
            .send()
            .map_err(|e| format!("GitLab commit: {e}"))?;

        if resp.status().is_success() {
            return Ok(());
        }

        // Fallback: try "master" as start_branch
        let body_master = serde_json::json!({
            "branch": "opensession",
            "start_branch": "master",
            "commit_message": "Add session",
            "actions": [{
                "action": "create",
                "file_path": path,
                "content": STANDARD.encode(content),
                "encoding": "base64"
            }]
        });

        let resp2 = self
            .gl_post(&format!("{base}/repository/commits"))
            .json(&body_master)
            .send()
            .map_err(|e| format!("GitLab commit (master): {e}"))?;

        if !resp2.status().is_success() {
            let status = resp2.status();
            let body = resp2.text().unwrap_or_default();
            return Err(format!("GitLab branch creation failed ({status}): {body}"));
        }
        Ok(())
    }
}

fn detect_platform(remote_url: &str) -> Result<Platform, String> {
    let normalized = remote_url
        .trim()
        .trim_end_matches(".git")
        .replace("git@github.com:", "https://github.com/")
        .replace("git@gitlab.com:", "https://gitlab.com/");

    if normalized.contains("github.com") {
        let path = normalized.trim_start_matches("https://github.com/");
        let (owner, repo) = path
            .split_once('/')
            .ok_or_else(|| format!("Cannot parse GitHub owner/repo from: {remote_url}"))?;
        Ok(Platform::GitHub {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    } else if normalized.contains("gitlab.com") {
        let path = normalized.trim_start_matches("https://gitlab.com/");
        if path.is_empty() || !path.contains('/') {
            return Err(format!(
                "Cannot parse GitLab project path from: {remote_url}"
            ));
        }
        Ok(Platform::GitLab {
            project_path: path.to_string(),
        })
    } else {
        Err(format!("Unsupported git platform: {remote_url}"))
    }
}

#[derive(Deserialize)]
struct ShaResponse {
    sha: String,
}
