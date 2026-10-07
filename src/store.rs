//! Local SQLite cache: what the window shows before the network answers,
//! and an FTS5 index for instant search over everything seen so far.
//! Rows keep the server's JSON; the indexed columns are only for lookups.

use crate::model::*;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct Store {
    db: Connection,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS teams (id TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS channels (id TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS members (channel_id TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS posts (
    id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    root_id TEXT NOT NULL,
    create_at INTEGER NOT NULL,
    json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS posts_channel ON posts (channel_id, root_id, create_at);
CREATE INDEX IF NOT EXISTS posts_root ON posts (root_id, create_at);
CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5 (id UNINDEXED, message, tokenize = 'unicode61');
";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = Connection::open(path)?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db })
    }

    pub fn get_kv(&self, k: &str) -> Option<String> {
        self.db.query_row("SELECT v FROM kv WHERE k = ?1", [k], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_kv(&self, k: &str, v: &str) {
        let _ = self.db.execute("INSERT OR REPLACE INTO kv (k, v) VALUES (?1, ?2)", params![k, v]);
    }

    fn all<T: serde::de::DeserializeOwned>(&self, table: &str) -> Vec<T> {
        let Ok(mut stmt) = self.db.prepare(&format!("SELECT json FROM {table}")) else {
            return Vec::new();
        };
        stmt.query_map([], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().filter_map(|j| serde_json::from_str(&j).ok()).collect())
            .unwrap_or_default()
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        let me: User = serde_json::from_str(&self.get_kv("me")?).ok()?;
        Some(Snapshot {
            me,
            teams: self.all("teams"),
            channels: self.all("channels"),
            members: self.all("members"),
            users: self.all("users"),
        })
    }

    pub fn save_snapshot(&mut self, snap: &Snapshot) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("INSERT OR REPLACE INTO kv (k, v) VALUES ('me', ?1)", [serde_json::to_string(&snap.me)?])?;
        tx.execute("DELETE FROM teams", [])?;
        tx.execute("DELETE FROM channels", [])?;
        tx.execute("DELETE FROM members", [])?;
        for t in &snap.teams {
            tx.execute("INSERT OR REPLACE INTO teams (id, json) VALUES (?1, ?2)", params![t.id, serde_json::to_string(t)?])?;
        }
        for c in &snap.channels {
            tx.execute("INSERT OR REPLACE INTO channels (id, json) VALUES (?1, ?2)", params![c.id, serde_json::to_string(c)?])?;
        }
        for m in &snap.members {
            tx.execute(
                "INSERT OR REPLACE INTO members (channel_id, json) VALUES (?1, ?2)",
                params![m.channel_id, serde_json::to_string(m)?],
            )?;
        }
        for u in &snap.users {
            tx.execute("INSERT OR REPLACE INTO users (id, json) VALUES (?1, ?2)", params![u.id, serde_json::to_string(u)?])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save_channel(&self, c: &Channel) {
        if let Ok(j) = serde_json::to_string(c) {
            let _ = self.db.execute("INSERT OR REPLACE INTO channels (id, json) VALUES (?1, ?2)", params![c.id, j]);
        }
    }

    pub fn save_member(&self, m: &ChannelMember) {
        if let Ok(j) = serde_json::to_string(m) {
            let _ = self
                .db
                .execute("INSERT OR REPLACE INTO members (channel_id, json) VALUES (?1, ?2)", params![m.channel_id, j]);
        }
    }

    pub fn save_users(&mut self, users: &[User]) {
        let Ok(tx) = self.db.transaction() else { return };
        for u in users {
            if let Ok(j) = serde_json::to_string(u) {
                let _ = tx.execute("INSERT OR REPLACE INTO users (id, json) VALUES (?1, ?2)", params![u.id, j]);
            }
        }
        let _ = tx.commit();
    }

    pub fn known_users(&self) -> std::collections::HashSet<String> {
        let Ok(mut stmt) = self.db.prepare("SELECT id FROM users") else {
            return Default::default();
        };
        stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    pub fn save_posts(&mut self, posts: &[Post]) {
        let Ok(tx) = self.db.transaction() else { return };
        for p in posts {
            if p.pending {
                continue;
            }
            if p.delete_at > 0 {
                let _ = tx.execute("DELETE FROM posts WHERE id = ?1", [&p.id]);
                let _ = tx.execute("DELETE FROM posts_fts WHERE id = ?1", [&p.id]);
                continue;
            }
            let Ok(j) = serde_json::to_string(p) else { continue };
            let _ = tx.execute(
                "INSERT OR REPLACE INTO posts (id, channel_id, root_id, create_at, json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![p.id, p.channel_id, p.root_id, p.create_at, j],
            );
            let _ = tx.execute("DELETE FROM posts_fts WHERE id = ?1", [&p.id]);
            if !p.message.is_empty() && !p.is_system() {
                let _ = tx.execute("INSERT INTO posts_fts (id, message) VALUES (?1, ?2)", params![p.id, p.message]);
            }
        }
        let _ = tx.commit();
    }

    pub fn delete_post(&self, id: &str) {
        let _ = self.db.execute("DELETE FROM posts WHERE id = ?1", [id]);
        let _ = self.db.execute("DELETE FROM posts_fts WHERE id = ?1", [id]);
    }

    pub fn post(&self, id: &str) -> Option<Post> {
        let j: String = self.db.query_row("SELECT json FROM posts WHERE id = ?1", [id], |r| r.get(0)).optional().ok()??;
        serde_json::from_str(&j).ok()
    }

    fn query_posts(&self, sql: &str, args: impl rusqlite::Params) -> Vec<Post> {
        let Ok(mut stmt) = self.db.prepare(sql) else { return Vec::new() };
        stmt.query_map(args, |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().filter_map(|j| serde_json::from_str(&j).ok()).collect())
            .unwrap_or_default()
    }

    /// The newest `limit` root posts of a channel, oldest first.
    pub fn recent_roots(&self, channel_id: &str, limit: usize) -> Vec<Post> {
        let mut posts = self.query_posts(
            "SELECT json FROM posts WHERE channel_id = ?1 AND root_id = '' ORDER BY create_at DESC LIMIT ?2",
            params![channel_id, limit as i64],
        );
        posts.reverse();
        posts
    }

    pub fn thread(&self, root_id: &str) -> Vec<Post> {
        let mut posts = self.query_posts("SELECT json FROM posts WHERE id = ?1", [root_id]);
        posts.extend(self.query_posts("SELECT json FROM posts WHERE root_id = ?1 ORDER BY create_at", [root_id]));
        posts
    }

    /// Drop cached root posts of a channel older than `before`: a gap opened
    /// between them and what the server just sent.
    pub fn forget_older(&self, channel_id: &str, before: i64) {
        let _ = self.db.execute(
            "DELETE FROM posts_fts WHERE id IN (SELECT id FROM posts WHERE channel_id = ?1 AND create_at < ?2)",
            params![channel_id, before],
        );
        let _ = self.db.execute("DELETE FROM posts WHERE channel_id = ?1 AND create_at < ?2", params![channel_id, before]);
    }

    pub fn newest_root_at(&self, channel_id: &str) -> Option<i64> {
        self.db
            .query_row(
                "SELECT MAX(create_at) FROM posts WHERE channel_id = ?1 AND root_id = ''",
                [channel_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
    }

    /// Instant search over cached posts. Words become prefix matches;
    /// Mattermost modifiers (`from:`, `in:` …) are left to the server.
    pub fn search(&self, terms: &str, limit: usize) -> Vec<Post> {
        let words: Vec<String> = terms
            .split_whitespace()
            .filter(|w| !w.contains(':') && !w.starts_with('-'))
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        let query = words.join(" ");
        self.query_posts(
            "SELECT p.json FROM posts_fts f JOIN posts p ON p.id = f.id WHERE posts_fts MATCH ?1 \
             ORDER BY p.create_at DESC LIMIT ?2",
            params![query, limit as i64],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, channel: &str, at: i64, msg: &str) -> Post {
        Post { id: id.into(), channel_id: channel.into(), create_at: at, message: msg.into(), ..Default::default() }
    }

    fn store() -> (Store, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("mmfast-test-{}-{n}", std::process::id()));
        (Store::open(&dir.join("cache.db")).unwrap(), dir)
    }

    #[test]
    fn search_finds_prefixes_and_skips_modifiers() {
        let (mut s, dir) = store();
        s.save_posts(&[post("a", "c", 1, "The 2D Alphabet fit converged"), post("b", "c", 2, "lunch at R1?")]);
        let hits: Vec<String> = s.search("alpha from:alice", 10).into_iter().map(|p| p.id).collect();
        assert_eq!(hits, vec!["a"]);
        assert!(s.search("from:alice", 10).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn deleted_posts_leave_cache_and_index() {
        let (mut s, dir) = store();
        s.save_posts(&[post("a", "c", 1, "condor queue stuck")]);
        let mut gone = post("a", "c", 1, "condor queue stuck");
        gone.delete_at = 5;
        s.save_posts(&[gone]);
        assert!(s.post("a").is_none());
        assert!(s.search("condor", 10).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn recent_roots_are_oldest_first_and_gap_forgets_older() {
        let (mut s, dir) = store();
        let mut reply = post("r", "c", 3, "a reply");
        reply.root_id = "a".into();
        s.save_posts(&[post("a", "c", 1, "one"), post("b", "c", 2, "two"), reply, post("x", "other", 9, "x")]);
        let ids: Vec<String> = s.recent_roots("c", 10).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["a", "b"]);
        s.forget_older("c", 2);
        let ids: Vec<String> = s.recent_roots("c", 10).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["b"]);
        assert_eq!(s.newest_root_at("c"), Some(2));
        std::fs::remove_dir_all(dir).ok();
    }
}
