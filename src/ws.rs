//! The Mattermost websocket: live posts, edits, reactions, typing, read
//! state. Reconnects with backoff; the worker catches up with REST after
//! every reconnect, so a missed event is never a missing message.

use crate::model::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

#[derive(Debug)]
pub enum WsEvent {
    Connected,
    Disconnected(String),
    Posted { post: Post, mentions: Vec<String> },
    PostEdited(Post),
    PostDeleted(Post),
    Reaction { reaction: Reaction, added: bool },
    Typing { channel_id: String, parent_id: String, user_id: String },
    /// Read elsewhere (one channel, or several at once).
    ChannelsViewed { channel_ids: Vec<String> },
    ChannelUpdated(Channel),
    /// Membership or channel list changed; the worker refetches the sidebar.
    Resync,
    UserUpdated(User),
    MemberUpdated(ChannelMember),
    CategoriesChanged { team_id: String },
    ThreadUpdated,
}

/// What the worker can send over the socket.
#[derive(Debug)]
pub enum WsOut {
    Typing { channel_id: String, parent_id: String },
}

fn json_field<T: serde::de::DeserializeOwned>(data: &Value, key: &str) -> Option<T> {
    match &data[key] {
        Value::String(s) => serde_json::from_str(s).ok(),
        Value::Null => None,
        other => serde_json::from_value(other.clone()).ok(),
    }
}

fn parse(text: &str) -> Option<WsEvent> {
    let msg: Value = serde_json::from_str(text).ok()?;
    let event = msg["event"].as_str()?;
    let data = &msg["data"];
    let broadcast = &msg["broadcast"];
    Some(match event {
        "posted" | "ephemeral_message" => WsEvent::Posted {
            post: json_field(data, "post")?,
            mentions: json_field(data, "mentions").unwrap_or_default(),
        },
        "post_edited" => WsEvent::PostEdited(json_field(data, "post")?),
        "post_deleted" => WsEvent::PostDeleted(json_field(data, "post")?),
        "reaction_added" => WsEvent::Reaction { reaction: json_field(data, "reaction")?, added: true },
        "reaction_removed" => WsEvent::Reaction { reaction: json_field(data, "reaction")?, added: false },
        "typing" => WsEvent::Typing {
            channel_id: broadcast["channel_id"].as_str()?.to_string(),
            parent_id: data["parent_id"].as_str().unwrap_or_default().to_string(),
            user_id: data["user_id"].as_str()?.to_string(),
        },
        "channel_viewed" => WsEvent::ChannelsViewed { channel_ids: vec![data["channel_id"].as_str()?.to_string()] },
        "multiple_channels_viewed" => {
            WsEvent::ChannelsViewed { channel_ids: data["channel_times"].as_object()?.keys().cloned().collect() }
        }
        "channel_updated" | "channel_converted" => WsEvent::ChannelUpdated(json_field(data, "channel")?),
        "channel_created" | "channel_deleted" | "user_added" | "user_removed" | "added_to_team"
        | "leave_team" | "direct_added" | "group_added" | "channel_restored" => WsEvent::Resync,
        "user_updated" => WsEvent::UserUpdated(json_field(data, "user")?),
        "channel_member_updated" => WsEvent::MemberUpdated(json_field(data, "channelMember")?),
        "sidebar_category_created" | "sidebar_category_updated" | "sidebar_category_deleted"
        | "sidebar_category_order_updated" => WsEvent::CategoriesChanged {
            team_id: broadcast["team_id"].as_str().or(data["team_id"].as_str()).unwrap_or_default().to_string(),
        },
        "thread_updated" => {
            let _: Value = json_field(data, "thread")?;
            WsEvent::ThreadUpdated
        }
        _ => return None,
    })
}

pub async fn run(url: String, token: String, events: mpsc::UnboundedSender<WsEvent>, mut out: mpsc::UnboundedReceiver<WsOut>) {
    let mut backoff = 1u64;
    let mut seq = 1u64;
    loop {
        let mut request = match url.clone().into_client_request() {
            Ok(r) => r,
            Err(err) => {
                let _ = events.send(WsEvent::Disconnected(err.to_string()));
                return;
            }
        };
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
            request.headers_mut().insert("Authorization", value);
        }
        match tokio_tungstenite::connect_async(request).await {
            Ok((stream, _)) => {
                backoff = 1;
                let (mut write, mut read) = stream.split();
                // Header auth is enough on current servers; the challenge
                // covers older ones and proxies that drop the header.
                let challenge = serde_json::json!({
                    "seq": seq, "action": "authentication_challenge", "data": { "token": token }
                });
                seq += 1;
                let _ = write.send(Message::text(challenge.to_string())).await;
                if events.send(WsEvent::Connected).is_err() {
                    return;
                }
                let mut ping = tokio::time::interval(Duration::from_secs(30));
                ping.tick().await;
                let reason = loop {
                    tokio::select! {
                        msg = read.next() => match msg {
                            Some(Ok(Message::Text(text))) => {
                                if let Some(ev) = parse(&text) && events.send(ev).is_err() {
                                    return;
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break "closed".to_string(),
                            Some(Err(err)) => break err.to_string(),
                            _ => {}
                        },
                        Some(cmd) = out.recv() => {
                            let WsOut::Typing { channel_id, parent_id } = cmd;
                            let msg = serde_json::json!({
                                "seq": seq, "action": "user_typing",
                                "data": { "channel_id": channel_id, "parent_id": parent_id }
                            });
                            seq += 1;
                            if write.send(Message::text(msg.to_string())).await.is_err() {
                                break "send failed".to_string();
                            }
                        }
                        _ = ping.tick() => {
                            if write.send(Message::Ping(Vec::new().into())).await.is_err() {
                                break "ping failed".to_string();
                            }
                        }
                    }
                };
                if events.send(WsEvent::Disconnected(reason)).is_err() {
                    return;
                }
            }
            Err(err) => {
                if events.send(WsEvent::Disconnected(err.to_string())).is_err() {
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}
