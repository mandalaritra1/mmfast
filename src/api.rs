//! Typed Mattermost REST v4 client. Runs on the backend runtime only.

use crate::model::*;
use anyhow::{Context as _, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::time::Duration;

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() {
            write!(f, "server answered {}", self.status)
        } else {
            write!(f, "{} ({})", self.message, self.status)
        }
    }
}

impl std::error::Error for ApiError {}

pub fn is_unauthorized(err: &anyhow::Error) -> bool {
    err.downcast_ref::<ApiError>().is_some_and(|e| e.status == 401)
}

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    pub base: String,
    token: String,
}

pub fn normalize_server(server: &str) -> String {
    let s = server.trim().trim_end_matches('/');
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("mmfast/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(8)
        .build()
        .expect("http client")
}

async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    Err(ApiError {
        status: status.as_u16(),
        message: parsed["message"].as_str().unwrap_or_default().to_string(),
    }
    .into())
}

impl Api {
    pub fn new(server: &str, token: &str) -> Self {
        Self { http: client(), base: normalize_server(server), token: token.to_string() }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Password (and optional MFA) login. Returns the session token and us.
    pub async fn login(server: &str, login_id: &str, password: &str, mfa: &str) -> Result<(String, User)> {
        let base = normalize_server(server);
        let mut body = json!({ "login_id": login_id, "password": password });
        if !mfa.trim().is_empty() {
            body["token"] = json!(mfa.trim());
        }
        let resp = client()
            .post(format!("{base}/api/v4/users/login"))
            .header("X-Requested-With", "XMLHttpRequest")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("could not reach {base}"))?;
        let resp = check(resp).await?;
        let token = resp
            .headers()
            .get("Token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("the server did not return a session token"))?
            .to_string();
        let user: User = resp.json().await?;
        Ok((token, user))
    }

    /// The server's public settings (no sign-in needed).
    pub async fn client_config(server: &str) -> Result<serde_json::Value> {
        let base = normalize_server(server);
        let resp = client()
            .get(format!("{base}/api/v4/config/client?format=old"))
            .send()
            .await
            .with_context(|| format!("could not reach {base}"))?;
        Ok(check(resp).await?.json().await?)
    }

    /// The browser page that starts single sign-on and comes back to
    /// `redirect` (an `mmauth://` link) with the session token.
    pub async fn sso_url(server: &str, redirect: &str) -> Result<String> {
        let base = normalize_server(server);
        let cfg = Self::client_config(server).await?;
        let on = |k: &str| cfg[k].as_str() == Some("true");
        let r = urlencode(redirect);
        for (key, service) in [
            ("EnableSignUpWithGitLab", "gitlab"),
            ("EnableSignUpWithOpenId", "openid"),
            ("EnableSignUpWithOffice365", "office365"),
            ("EnableSignUpWithGoogle", "google"),
        ] {
            if on(key) {
                return Ok(format!("{base}/oauth/{service}/mobile_login?redirect_to={r}"));
            }
        }
        if on("EnableSaml") {
            return Ok(format!("{base}/login/sso/saml?action=mobile&redirect_to={r}"));
        }
        bail!("{base} has no single sign-on; use a password or a token")
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.base, path)
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.url(path))
            .bearer_auth(&self.token)
            .header("X-Requested-With", "XMLHttpRequest")
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = check(self.req(reqwest::Method::GET, path).send().await?).await?;
        Ok(resp.json().await?)
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let resp = check(self.req(method, path).json(body).send().await?).await?;
        let bytes = resp.bytes().await?;
        if bytes.is_empty() {
            return Ok(serde_json::from_value(serde_json::Value::Null)?);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> Result<T> {
        self.send_json(reqwest::Method::POST, path, &body).await
    }

    async fn put<T: DeserializeOwned>(&self, path: &str, body: serde_json::Value) -> Result<T> {
        self.send_json(reqwest::Method::PUT, path, &body).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        check(self.req(reqwest::Method::DELETE, path).send().await?).await?;
        Ok(())
    }

    pub async fn bytes(&self, path: &str) -> Result<Vec<u8>> {
        let resp = check(self.req(reqwest::Method::GET, path).send().await?).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    // ------------------------------------------------------------ session

    pub async fn me(&self) -> Result<User> {
        self.get("/users/me").await
    }

    pub async fn logout(&self) -> Result<()> {
        let _: serde_json::Value = self.post("/users/logout", json!({})).await?;
        Ok(())
    }

    // ------------------------------------------------------------ sidebar

    pub async fn my_teams(&self) -> Result<Vec<Team>> {
        self.get("/users/me/teams").await
    }

    pub async fn my_channels(&self, team_id: &str) -> Result<Vec<Channel>> {
        self.get(&format!("/users/me/teams/{team_id}/channels?include_deleted=false")).await
    }

    pub async fn my_members(&self, team_id: &str) -> Result<Vec<ChannelMember>> {
        self.get(&format!("/users/me/teams/{team_id}/channels/members")).await
    }

    pub async fn users_by_ids(&self, ids: &[String]) -> Result<Vec<User>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.post("/users/ids", json!(ids)).await
    }

    /// Of `ids`, the users changed since `since` (ms).
    pub async fn users_changed_since(&self, ids: &[String], since: i64) -> Result<Vec<User>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.post(&format!("/users/ids?since={since}"), json!(ids)).await
    }

    pub async fn autocomplete_users(&self, team_id: &str, channel_id: &str, name: &str) -> Result<Vec<User>> {
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct Auto {
            users: Vec<User>,
            out_of_channel: Option<Vec<User>>,
        }
        let path = format!(
            "/users/autocomplete?in_team={team_id}&in_channel={channel_id}&name={}&limit=25",
            urlencode(name)
        );
        let auto: Auto = self.get(&path).await?;
        let mut users = auto.users;
        users.extend(auto.out_of_channel.unwrap_or_default());
        Ok(users)
    }

    pub async fn view_channel(&self, channel_id: &str) -> Result<()> {
        let _: serde_json::Value =
            self.post("/channels/members/me/view", json!({ "channel_id": channel_id, "collapsed_threads_supported": true })).await?;
        Ok(())
    }

    pub async fn categories(&self, team_id: &str) -> Result<Vec<Category>> {
        let cats: Categories = self.get(&format!("/users/me/teams/{team_id}/channels/categories")).await?;
        Ok(cats.ordered())
    }

    pub async fn update_categories(&self, user_id: &str, team_id: &str, cats: &[Category]) -> Result<()> {
        let _: serde_json::Value =
            self.put(&format!("/users/{user_id}/teams/{team_id}/channels/categories"), json!(cats)).await?;
        Ok(())
    }

    /// Public channels of a team, or those matching `term`.
    pub async fn browse_channels(&self, team_id: &str, term: &str) -> Result<Vec<Channel>> {
        if term.trim().is_empty() {
            self.get(&format!("/teams/{team_id}/channels?page=0&per_page=200")).await
        } else {
            self.post(&format!("/teams/{team_id}/channels/search"), json!({ "term": term.trim() })).await
        }
    }

    pub async fn channel(&self, channel_id: &str) -> Result<Channel> {
        self.get(&format!("/channels/{channel_id}")).await
    }

    pub async fn member(&self, channel_id: &str) -> Result<ChannelMember> {
        self.get(&format!("/channels/{channel_id}/members/me")).await
    }

    pub async fn join_channel(&self, channel_id: &str, user_id: &str) -> Result<()> {
        let _: serde_json::Value = self.post(&format!("/channels/{channel_id}/members"), json!({ "user_id": user_id })).await?;
        Ok(())
    }

    pub async fn leave_channel(&self, channel_id: &str, user_id: &str) -> Result<()> {
        self.delete(&format!("/channels/{channel_id}/members/{user_id}")).await
    }

    pub async fn direct_channel(&self, me: &str, other: &str) -> Result<Channel> {
        self.post("/channels/direct", json!([me, other])).await
    }

    pub async fn set_notify_props(&self, channel_id: &str, user_id: &str, props: serde_json::Value) -> Result<()> {
        let mut props = props;
        props["channel_id"] = json!(channel_id);
        props["user_id"] = json!(user_id);
        let _: serde_json::Value = self.put(&format!("/channels/{channel_id}/members/{user_id}/notify_props"), props).await?;
        Ok(())
    }

    /// Every custom emoji on the server, as (name, id).
    pub async fn custom_emoji(&self) -> Result<Vec<(String, String)>> {
        #[derive(serde::Deserialize)]
        struct E {
            id: String,
            name: String,
        }
        let mut out = Vec::new();
        for page in 0..50 {
            let list: Vec<E> = self.get(&format!("/emoji?page={page}&per_page=200&sort=name")).await?;
            let n = list.len();
            out.extend(list.into_iter().map(|e| (e.name, e.id)));
            if n < 200 {
                break;
            }
        }
        Ok(out)
    }

    pub async fn channel_stats(&self, channel_id: &str) -> Result<ChannelStats> {
        self.get(&format!("/channels/{channel_id}/stats")).await
    }

    pub async fn patch_channel_header(&self, channel_id: &str, header: &str) -> Result<Channel> {
        self.put(&format!("/channels/{channel_id}/patch"), json!({ "header": header })).await
    }

    pub async fn pinned(&self, channel_id: &str) -> Result<Vec<Post>> {
        let list: PostList = self.get(&format!("/channels/{channel_id}/pinned")).await?;
        Ok(list.into_ordered())
    }

    // ------------------------------------------------------------ posts

    /// Root posts of a channel (collapsed threads), newest first.
    pub async fn channel_posts(&self, channel_id: &str, query: &str) -> Result<Vec<Post>> {
        let path = format!(
            "/channels/{channel_id}/posts?collapsedThreads=true&collapsedThreadsExtended=false&{query}"
        );
        let list: PostList = self.get(&path).await?;
        Ok(list.into_ordered())
    }

    pub async fn thread(&self, root_id: &str) -> Result<Vec<Post>> {
        let list: PostList = self.get(&format!("/posts/{root_id}/thread")).await?;
        Ok(list.into_ordered())
    }

    pub async fn post_by_id(&self, post_id: &str) -> Result<Post> {
        self.get(&format!("/posts/{post_id}")).await
    }

    pub async fn create_post(
        &self,
        channel_id: &str,
        root_id: &str,
        message: &str,
        pending_post_id: &str,
        file_ids: &[String],
    ) -> Result<Post> {
        self.post(
            "/posts",
            json!({
                "channel_id": channel_id,
                "root_id": root_id,
                "message": message,
                "pending_post_id": pending_post_id,
                "file_ids": file_ids,
            }),
        )
        .await
    }

    pub async fn edit_post(&self, post_id: &str, message: &str) -> Result<Post> {
        self.put(&format!("/posts/{post_id}/patch"), json!({ "message": message })).await
    }

    pub async fn delete_post(&self, post_id: &str) -> Result<()> {
        self.delete(&format!("/posts/{post_id}")).await
    }

    pub async fn set_pinned(&self, post_id: &str, pinned: bool) -> Result<()> {
        let action = if pinned { "pin" } else { "unpin" };
        let _: serde_json::Value = self.post(&format!("/posts/{post_id}/{action}"), json!({})).await?;
        Ok(())
    }

    pub async fn add_reaction(&self, user_id: &str, post_id: &str, emoji: &str) -> Result<()> {
        let _: serde_json::Value = self
            .post("/reactions", json!({ "user_id": user_id, "post_id": post_id, "emoji_name": emoji }))
            .await?;
        Ok(())
    }

    pub async fn remove_reaction(&self, user_id: &str, post_id: &str, emoji: &str) -> Result<()> {
        self.delete(&format!("/users/{user_id}/posts/{post_id}/reactions/{emoji}")).await
    }

    // ------------------------------------------------------------ threads

    pub async fn user_threads(&self, user_id: &str, team_id: &str) -> Result<UserThreads> {
        self.get(&format!("/users/{user_id}/teams/{team_id}/threads?extended=true&per_page=50")).await
    }

    pub async fn mark_thread_read(&self, user_id: &str, team_id: &str, thread_id: &str) -> Result<()> {
        let now = now_ms();
        let _: serde_json::Value = self
            .put(&format!("/users/{user_id}/teams/{team_id}/threads/{thread_id}/read/{now}"), json!({}))
            .await?;
        Ok(())
    }

    // ------------------------------------------------------------ search

    pub async fn search_posts(&self, team_id: &str, terms: &str) -> Result<Vec<Post>> {
        let offset = jiff::Zoned::now().offset().seconds();
        let list: PostList = self
            .post(
                &format!("/teams/{team_id}/posts/search"),
                json!({
                    "terms": terms,
                    "is_or_search": false,
                    "time_zone_offset": offset,
                    "include_deleted_channels": false,
                    "page": 0,
                    "per_page": 60,
                }),
            )
            .await?;
        Ok(list.into_ordered())
    }

    pub async fn search_files(&self, team_id: &str, terms: &str) -> Result<Vec<FileInfo>> {
        let list: FileInfoList = self
            .post(
                &format!("/teams/{team_id}/files/search"),
                json!({ "terms": terms, "is_or_search": false, "page": 0, "per_page": 60 }),
            )
            .await?;
        let mut out = Vec::new();
        let mut infos = list.file_infos;
        for id in list.order {
            if let Some(f) = infos.remove(&id) {
                out.push(f);
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ files

    pub async fn upload(&self, channel_id: &str, name: &str, data: Vec<u8>) -> Result<FileInfo> {
        #[derive(serde::Deserialize)]
        struct Uploaded {
            file_infos: Vec<FileInfo>,
        }
        let mime = guess_mime(name);
        let part = reqwest::multipart::Part::bytes(data).file_name(name.to_string()).mime_str(mime)?;
        let form = reqwest::multipart::Form::new().text("channel_id", channel_id.to_string()).part("files", part);
        let resp = check(self.req(reqwest::Method::POST, "/files").multipart(form).send().await?).await?;
        let up: Uploaded = resp.json().await?;
        up.file_infos.into_iter().next().ok_or_else(|| anyhow!("upload returned no file"))
    }

    pub async fn file_bytes(&self, key: &ImageKey) -> Result<Vec<u8>> {
        match key {
            ImageKey::Avatar { user_id, .. } => self.bytes(&format!("/users/{user_id}/image")).await,
            ImageKey::TeamIcon { team_id, .. } => self.bytes(&format!("/teams/{team_id}/image")).await,
            ImageKey::Emoji(id) => self.bytes(&format!("/emoji/{id}/image")).await,
            ImageKey::Preview(id) => match self.bytes(&format!("/files/{id}/preview")).await {
                Ok(b) => Ok(b),
                // Small images and GIFs have no generated preview.
                Err(_) => self.bytes(&format!("/files/{id}")).await,
            },
            ImageKey::Full(id) | ImageKey::Animated(id) => self.bytes(&format!("/files/{id}")).await,
        }
    }

    pub async fn download(&self, file_id: &str) -> Result<Vec<u8>> {
        self.bytes(&format!("/files/{file_id}")).await
    }

    pub fn websocket_url(&self) -> Result<String> {
        let base = &self.base;
        let ws = if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            bail!("unsupported server address {base}");
        };
        Ok(format!("{ws}/api/v4/websocket"))
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

pub fn guess_mime(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}
