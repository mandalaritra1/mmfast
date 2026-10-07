//! The app's own types. They deserialize straight from Mattermost's REST v4
//! JSON; every field defaults so a newer or older server never fails a parse.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
    pub id: String,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub nickname: String,
    pub last_picture_update: i64,
    pub roles: String,
    pub delete_at: i64,
    pub is_bot: bool,
    pub notify_props: serde_json::Value,
    pub position: String,
    pub email: String,
}

impl User {
    pub fn display_name(&self) -> String {
        let full = format!("{} {}", self.first_name, self.last_name);
        let full = full.trim();
        if full.is_empty() {
            self.username.clone()
        } else {
            full.to_string()
        }
    }

    pub fn is_admin(&self) -> bool {
        self.roles.split_whitespace().any(|r| r == "system_admin")
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub last_team_icon_update: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Channel {
    pub id: String,
    pub team_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub display_name: String,
    pub header: String,
    pub purpose: String,
    pub last_post_at: i64,
    pub last_root_post_at: i64,
    pub total_msg_count: i64,
    pub total_msg_count_root: i64,
    pub delete_at: i64,
    pub creator_id: String,
}

impl Channel {
    pub fn is_direct(&self) -> bool {
        self.kind == "D" || self.kind == "G"
    }

    /// For a DM, the other member's id (or our own for a self-DM).
    pub fn dm_partner(&self, me: &str) -> Option<String> {
        if self.kind != "D" {
            return None;
        }
        let mut parts = self.name.split("__");
        let a = parts.next()?;
        let b = parts.next()?;
        Some(if a == me { b } else { a }.to_string())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelMember {
    pub channel_id: String,
    pub user_id: String,
    pub msg_count: i64,
    pub msg_count_root: i64,
    pub mention_count: i64,
    pub mention_count_root: i64,
    pub last_viewed_at: i64,
    pub roles: String,
    pub notify_props: serde_json::Value,
}

impl ChannelMember {
    /// Muted: unread only when mentioned, no notifications.
    pub fn muted(&self) -> bool {
        self.notify_props["mark_unread"].as_str() == Some("mention")
    }
}

/// A sidebar category (Favorites, Channels, Direct Messages, or the user's own).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Category {
    pub id: String,
    pub user_id: String,
    pub team_id: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub sorting: String,
    pub channel_ids: Vec<String>,
    pub collapsed: bool,
    pub muted: bool,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Categories {
    pub categories: Vec<Category>,
    pub order: Vec<String>,
}

impl Categories {
    /// Categories in the user's order.
    pub fn ordered(mut self) -> Vec<Category> {
        let order = std::mem::take(&mut self.order);
        let mut cats = self.categories;
        cats.sort_by_key(|c| order.iter().position(|o| o == &c.id).unwrap_or(usize::MAX));
        cats
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FileInfo {
    pub id: String,
    pub post_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub name: String,
    pub extension: String,
    pub size: i64,
    pub mime_type: String,
    pub width: i64,
    pub height: i64,
    pub has_preview_image: bool,
    pub create_at: i64,
}

impl FileInfo {
    pub fn is_gif(&self) -> bool {
        self.mime_type == "image/gif" || self.extension.eq_ignore_ascii_case("gif")
    }

    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/") && self.width > 0 && self.height > 0
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Reaction {
    pub user_id: String,
    pub post_id: String,
    pub emoji_name: String,
    pub create_at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PostMetadata {
    pub files: Vec<FileInfo>,
    pub reactions: Vec<Reaction>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Post {
    pub id: String,
    pub create_at: i64,
    pub update_at: i64,
    pub edit_at: i64,
    pub delete_at: i64,
    pub is_pinned: bool,
    pub user_id: String,
    pub channel_id: String,
    pub root_id: String,
    pub message: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub props: serde_json::Value,
    pub file_ids: Vec<String>,
    pub pending_post_id: String,
    pub reply_count: i64,
    pub last_reply_at: i64,
    pub participants: Option<Vec<User>>,
    pub metadata: PostMetadata,
    /// Ours: sent but not yet confirmed by the server.
    #[serde(skip)]
    pub pending: bool,
    /// Ours: the send failed; shown with a retry.
    #[serde(skip)]
    pub failed: bool,
}

impl Post {
    pub fn is_system(&self) -> bool {
        self.kind.starts_with("system_")
    }

}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PostList {
    pub order: Vec<String>,
    pub posts: HashMap<String, Post>,
    pub next_post_id: String,
    pub prev_post_id: String,
    pub has_next: bool,
}

impl PostList {
    /// Posts in `order` (newest first, as the server sends them).
    pub fn into_ordered(mut self) -> Vec<Post> {
        let mut out = Vec::with_capacity(self.order.len());
        for id in &self.order {
            if let Some(p) = self.posts.remove(id) {
                out.push(p);
            }
        }
        // Posts not named in order (thread replies in a thread fetch).
        out.extend(self.posts.into_values());
        out
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserThread {
    pub id: String,
    pub reply_count: i64,
    pub last_reply_at: i64,
    pub last_viewed_at: i64,
    pub participants: Vec<User>,
    pub post: Post,
    pub unread_replies: i64,
    pub unread_mentions: i64,
    pub is_following: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserThreads {
    pub total: i64,
    pub total_unread_threads: i64,
    pub total_unread_mentions: i64,
    pub threads: Vec<UserThread>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelStats {
    pub channel_id: String,
    pub member_count: i64,
    pub pinnedpost_count: i64,
    pub files_count: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileInfoList {
    pub order: Vec<String>,
    pub file_infos: HashMap<String, FileInfo>,
}

/// Everything the sidebar needs, as one snapshot.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub me: User,
    pub teams: Vec<Team>,
    pub channels: Vec<Channel>,
    pub members: Vec<ChannelMember>,
    pub users: Vec<User>,
}

/// Images the UI asks the backend for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageKey {
    Avatar { user_id: String, updated: i64 },
    TeamIcon { team_id: String, updated: i64 },
    Emoji(String),
    /// An animated GIF attachment: all its frames.
    Animated(String),
    Preview(String),
    Full(String),
}

impl ImageKey {
    pub fn cache_name(&self) -> String {
        match self {
            ImageKey::Avatar { user_id, updated } => format!("avatar_{user_id}_{updated}"),
            ImageKey::TeamIcon { team_id, updated } => format!("team_{team_id}_{updated}"),
            ImageKey::Emoji(id) => format!("emoji_{id}"),
            ImageKey::Animated(id) => format!("full_{id}"),
            ImageKey::Preview(id) => format!("preview_{id}"),
            ImageKey::Full(id) => format!("full_{id}"),
        }
    }
}

pub fn human_size(bytes: i64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else {
        format!("{:.1} GB", b / 1024.0 / 1024.0 / 1024.0)
    }
}
