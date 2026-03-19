//
// https://www.github.com/8go/matrix-commander-rs
// listen.rs
//

//! Module that bundles code together that uses the `matrix-sdk` API.
//! Primarily the matrix_sdk::Client API
//! (see <https://docs.rs/matrix-sdk/latest/matrix_sdk/struct.Client.html>).
//! This module implements the matrix-sdk-based portions of the primitives
//! 'listen', i.e. receiving and listening.

//use std::borrow::Cow;
// use std::env;
//use std::fs;
// use std::fs::File;
// use std::io::{self, Write};
// use std::ops::Deref;
// use std::path::Path;
//use serde::{Deserialize, Serialize};
//use serde_json::Result;
// use std::path::PathBuf;
use chrono::{DateTime, Local};
use tracing::{debug, error, info, warn};
// use thiserror::Error;
// use directories::ProjectDirs;
// use serde::{Deserialize, Serialize};

use matrix_sdk::{
    config::SyncSettings,
    event_handler::Ctx,
    // SessionMeta,
    room::MessagesOptions,
    // room,
    room::Room,
    // ruma::
    ruma::{
        api::client::{
            filter::{FilterDefinition, /* LazyLoadOptions, */ RoomEventFilter, RoomFilter},
            sync::sync_events::v3::Filter,
            // sync::sync_events,
        },
        events::room::encrypted::{
            OriginalSyncRoomEncryptedEvent,
            /* RoomEncryptedEventContent, */ SyncRoomEncryptedEvent,
        },
        events::room::message::{
            AudioMessageEventContent,
            // EmoteMessageEventContent,
            FileMessageEventContent,
            ImageMessageEventContent,
            MessageType,
            // NoticeMessageEventContent,
            // OriginalRoomMessageEvent, OriginalSyncRoomMessageEvent,
            // RedactedRoomMessageEventContent, RoomMessageEvent,
            // OriginalSyncRoomEncryptedEvent,
            RedactedSyncRoomMessageEvent,
            RoomMessageEventContent,
            SyncRoomMessageEvent,
            TextMessageEventContent,
            VideoMessageEventContent,
        },
        events::room::redaction::{
            OriginalSyncRoomRedactionEvent, RedactedSyncRoomRedactionEvent, SyncRoomRedactionEvent,
        },
        events::{
            AnySyncMessageLikeEvent,
            AnySyncTimelineEvent,
            OriginalSyncMessageLikeEvent,
            // OriginalMessageLikeEvent, // MessageLikeEventContent,
            SyncMessageLikeEvent,
        },
        // OwnedRoomAliasId,
        OwnedRoomId,
        OwnedUserId,
        // serde::Raw,
        // events::OriginalMessageLikeEvent,
        RoomId,
        // UserId,
        // OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName,
        // device_id, room_id, session_id, user_id, OwnedDeviceId, OwnedUserId,
        UInt,
    },
    Client,
};

use crate::types::{Error, Output};

/// Format timestamp from milliseconds since Unix epoch to a human-readable local time string
fn format_ts_millis(ts_ms: u64) -> String {
    let secs = (ts_ms / 1000) as i64;
    let nsecs = ((ts_ms % 1000) * 1_000_000) as u32;
    match DateTime::from_timestamp(secs, nsecs) {
        Some(dt) => dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string(),
        None => ts_ms.to_string(),
    }
}

/// Augment a serialized event JSON with extra fields (event_id, sender, origin_server_ts, room_id).
/// Serializes content once to a string, then injects extra fields via string manipulation
/// to avoid the overhead of serializing to an intermediate serde_json::Value and then
/// re-serializing to a string.
fn augment_event_json<S: serde::Serialize>(
    content: &S,
    event_id: &str,
    sender: &str,
    origin_server_ts: u64,
    room_id: &str,
) -> String {
    match serde_json::to_string(content) {
        Ok(json_str) => {
            if json_str.starts_with('{') {
                fn escape_json_str(s: &str) -> String {
                    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s))
                }
                let extra = format!(
                    "\"event_id\":{},\"sender\":{},\"origin_server_ts\":{},\"room_id\":{},",
                    escape_json_str(event_id),
                    escape_json_str(sender),
                    origin_server_ts,
                    escape_json_str(room_id),
                );
                let mut result = String::with_capacity(json_str.len() + extra.len());
                result.push('{');
                result.push_str(&extra);
                result.push_str(&json_str[1..]);
                result
            } else {
                json_str
            }
        }
        Err(e) => {
            warn!("augment_event_json: serialization failed: {}", e);
            e.to_string()
        }
    }
}

/// Lower-level utility function to handle originalsyncmessagelikeevent
fn handle_originalsyncmessagelikeevent(
    ev: &OriginalSyncMessageLikeEvent<RoomMessageEventContent>,
    room_id: &OwnedRoomId,
    room_nick: &str,
    context: &Ctx<EvHandlerContext>,
) {
    // --output json is handled above this level,
    // if Json is output this event processing is never needed and never reached
    debug!(
        "New message: {:?} from sender {:?}, room {:?}, event_id {:?}",
        ev.content,
        ev.sender,
        room_id, // ev does not contain room!
        ev.event_id,
    );
    if context.whoami != ev.sender || context.listen_self {
        let sender_nick = ev.sender.localpart();
        let datetime = format_ts_millis(u64::from(ev.origin_server_ts.0));
        let event_id_detail = if context.print_event_id {
            format!(" | {}", ev.event_id)
        } else {
            String::new()
        };

        match ev.content.msgtype.to_owned() {
            MessageType::Text(textmessageeventcontent) => {
                let TextMessageEventContent { body, .. } = textmessageeventcontent;
                // Match Python format:
                // Message received for room {room_nick} [{room_id}] | sender {sender_nick} [{sender}] | {datetime} [| {event_id}] | {body}
                println!(
                    "Message received for room {} [{}] | sender {} [{}] | {}{} | {}",
                    room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, body,
                );
            }
            MessageType::File(filemessageeventcontent) => {
                let FileMessageEventContent { body, .. } = filemessageeventcontent;
                println!(
                    "Message received for room {} [{}] | sender {} [{}] | {}{} | {}",
                    room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, body,
                );
            }
            MessageType::Image(imagemessageeventcontent) => {
                let ImageMessageEventContent { body, .. } = imagemessageeventcontent;
                println!(
                    "Message received for room {} [{}] | sender {} [{}] | {}{} | {}",
                    room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, body,
                );
            }
            MessageType::Audio(audiomessageeventcontent) => {
                let AudioMessageEventContent { body, .. } = audiomessageeventcontent;
                println!(
                    "Message received for room {} [{}] | sender {} [{}] | {}{} | {}",
                    room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, body,
                );
            }
            MessageType::Video(videomessageeventcontent) => {
                let VideoMessageEventContent { body, .. } = videomessageeventcontent;
                println!(
                    "Message received for room {} [{}] | sender {} [{}] | {}{} | {}",
                    room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, body,
                );
            }
            _ => {
                // Handle all other message types (Emote, Notice, Location, etc.)
                let body = ev.content.body();
                println!(
                    "Message received for room {} [{}] | sender {} [{}] | {}{} | {}",
                    room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, body,
                );
            }
        }
    } else {
        debug!("Skipping message from itself because --listen-self is not set.");
    }
}

/// Utility function to handle RedactedSyncRoomMessageEvent events.
// None of the args can be borrowed because this function is passed into a spawned process.
async fn handle_redactedsyncroommessageevent(
    ev: RedactedSyncRoomMessageEvent,
    room: Room,
    _client: Client,
    context: Ctx<EvHandlerContext>,
) {
    debug!(
        "Received a message for RedactedSyncRoomMessageEvent. {:?}",
        ev
    );
    if context.whoami == ev.sender && !context.listen_self {
        debug!("Skipping message from itself because --listen-self is not set.");
        return;
    }
    if !context.output.is_text() {
        // Serialize it to a JSON string.
        let j = augment_event_json(
            &ev.content, ev.event_id.as_str(), ev.sender.as_str(),
            u64::from(ev.origin_server_ts.0), room.room_id().as_str(),
        );
        println!("{}", j);
        return;
    }
    // Text format for redacted messages, matching Python's RedactedEvent format
    let room_id = room.room_id();
    let room_nick = room.cached_display_name()
        .map(|dn| dn.to_string())
        .unwrap_or_default();
    let sender_nick = ev.sender.localpart();
    let datetime = format_ts_millis(u64::from(ev.origin_server_ts.0));
    let event_id_detail = if context.print_event_id {
        format!(" | {}", ev.event_id)
    } else {
        String::new()
    };
    println!(
        "Message received for room {} [{}] | sender {} [{}] | {}{} | Received redacted event: sender: {}",
        room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail, ev.sender,
    );
}

fn handle_originalsyncroomredactionevent(ev: OriginalSyncRoomRedactionEvent, room: Room) {
    debug!(
        "Received a message for OriginalSyncRoomRedactionEvent. {:?}",
        ev
    );
    let j = augment_event_json(
        &ev.content, ev.event_id.as_str(), ev.sender.as_str(),
        u64::from(ev.origin_server_ts.0), room.room_id().as_str(),
    );
    println!("{}", j);
}

fn handle_redactedsyncroomredactionevent(ev: RedactedSyncRoomRedactionEvent, room: Room) {
    debug!(
        "Received a message for RedactedSyncRoomRedactionEvent. {:?}",
        ev
    );
    let j = augment_event_json(
        &ev.content, ev.event_id.as_str(), ev.sender.as_str(),
        u64::from(ev.origin_server_ts.0), room.room_id().as_str(),
    );
    println!("{}", j);
}

/// Utility function to handle SyncRoomRedactionEvent events.
// None of the args can be borrowed because this function is passed into a spawned process.
async fn handle_syncroomredactedevent(
    ev: SyncRoomRedactionEvent,
    room: Room,
    _client: Client,
    context: Ctx<EvHandlerContext>,
) {
    debug!("Received a message for SyncRoomRedactionEvent. {:?}", ev);
    if context.whoami == ev.sender() && !context.listen_self {
        debug!("Skipping message from itself because --listen-self is not set.");
        return;
    }
    if !context.output.is_text() {
        // Serialize it to a JSON string.
        match ev {
            SyncRoomRedactionEvent::Original(evi) => {
                handle_originalsyncroomredactionevent(evi, room)
            }
            SyncRoomRedactionEvent::Redacted(evi) => {
                handle_redactedsyncroomredactionevent(evi, room)
            }
        }
        return;
    }
    // Text format for redaction events, matching Python's RedactionEvent format
    let room_id = room.room_id();
    let room_nick = room.cached_display_name()
        .map(|dn| dn.to_string())
        .unwrap_or_default();
    let sender_nick = ev.sender().localpart();
    let datetime = format_ts_millis(u64::from(ev.origin_server_ts().0));
    let event_id_detail = if context.print_event_id {
        format!(" | {}", ev.event_id())
    } else {
        String::new()
    };
    println!(
        "Message received for room {} [{}] | sender {} [{}] | {}{} | Received redaction event: sender: {}",
        room_nick, room_id, sender_nick, ev.sender(), datetime, event_id_detail, ev.sender(),
    );
}

/// Utility function to handle SyncRoomEncryptedEvent events.
/// These fire when an encrypted message could not be decrypted by matrix-sdk.
// None of the args can be borrowed because this function is passed into a spawned process.
async fn handle_syncroomencryptedevent(
    ev: SyncRoomEncryptedEvent,
    room: Room,
    _client: Client,
    context: Ctx<EvHandlerContext>,
) {
    debug!("Received a SyncRoomEncryptedEvent message {:?}", ev);
    if context.whoami == ev.sender() && !context.listen_self {
        debug!("Skipping message from itself because --listen-self is not set.");
        return;
    }
    match ev {
        SyncMessageLikeEvent::Original(original_ev) => {
            if !context.output.is_text() {
                let j = augment_event_json(
                    &original_ev.content, original_ev.event_id.as_str(), original_ev.sender.as_str(),
                    u64::from(original_ev.origin_server_ts.0), room.room_id().as_str(),
                );
                println!("{}", j);
                return;
            }
            let room_id = room.room_id();
            let room_nick = room.cached_display_name()
                .map(|dn| dn.to_string())
                .unwrap_or_default();
            let sender_nick = original_ev.sender.localpart();
            let datetime = format_ts_millis(u64::from(original_ev.origin_server_ts.0));
            let event_id_detail = if context.print_event_id {
                format!(" | {}", original_ev.event_id)
            } else {
                String::new()
            };
            println!(
                "Message received for room {} [{}] | sender {} [{}] | {}{} | Encrypted message could not be decrypted",
                room_nick, room_id, sender_nick, original_ev.sender, datetime, event_id_detail,
            );
        }
        _ => {
            debug!("Received redacted encrypted event, skipping: {:?}", ev);
        }
    }
}

/// Utility function to handle OriginalSyncRoomEncryptedEvent events.
/// These fire when an encrypted message could not be decrypted by matrix-sdk.
// None of the args can be borrowed because this function is passed into a spawned process.
async fn handle_originalsyncroomencryptedevent(
    ev: OriginalSyncRoomEncryptedEvent,
    room: Room,
    _client: Client,
    context: Ctx<EvHandlerContext>,
) {
    debug!("Received a OriginalSyncRoomEncryptedEvent message {:?}", ev);
    if context.whoami == ev.sender && !context.listen_self {
        debug!("Skipping message from itself because --listen-self is not set.");
        return;
    }
    if !context.output.is_text() {
        let j = augment_event_json(
            &ev.content, ev.event_id.as_str(), ev.sender.as_str(),
            u64::from(ev.origin_server_ts.0), room.room_id().as_str(),
        );
        println!("{}", j);
        return;
    }
    let room_id = room.room_id();
    let room_nick = room.cached_display_name()
        .map(|dn| dn.to_string())
        .unwrap_or_default();
    let sender_nick = ev.sender.localpart();
    let datetime = format_ts_millis(u64::from(ev.origin_server_ts.0));
    let event_id_detail = if context.print_event_id {
        format!(" | {}", ev.event_id)
    } else {
        String::new()
    };
    println!(
        "Message received for room {} [{}] | sender {} [{}] | {}{} | Encrypted message could not be decrypted",
        room_nick, room_id, sender_nick, ev.sender, datetime, event_id_detail,
    );
}

/// Utility function to handle SyncRoomMessageEvent events.
// None of the args can be borrowed because this function is passed into a spawned process.
async fn handle_syncroommessageevent(
    ev: SyncRoomMessageEvent,
    room: Room,
    _client: Client,
    context: Ctx<EvHandlerContext>,
) {
    debug!("Received a message for event SyncRoomMessageEvent {:?}", ev);
    if context.whoami == ev.sender() && !context.listen_self {
        debug!("Skipping message from itself because --listen-self is not set.");
        return;
    }
    match ev {
        SyncMessageLikeEvent::Original(orginialmessagelikeevent) => {
            if !context.output.is_text() {
                let j = augment_event_json(
                    &orginialmessagelikeevent.content, orginialmessagelikeevent.event_id.as_str(),
                    orginialmessagelikeevent.sender.as_str(),
                    u64::from(orginialmessagelikeevent.origin_server_ts.0), room.room_id().as_str(),
                );
                println!("{}", j);
                return;
            }
            let room_nick = room.cached_display_name()
                .map(|dn| dn.to_string())
                .unwrap_or_default();
            handle_originalsyncmessagelikeevent(
                &orginialmessagelikeevent,
                &RoomId::parse(room.room_id()).unwrap(),
                &room_nick,
                &context,
            );
        }
        SyncMessageLikeEvent::Redacted(redacted) => {
            if !context.output.is_text() {
                let j = augment_event_json(
                    &redacted.content, redacted.event_id.as_str(), redacted.sender.as_str(),
                    u64::from(redacted.origin_server_ts.0), room.room_id().as_str(),
                );
                println!("{}", j);
                return;
            }
            let room_id = room.room_id();
            let room_nick = room.cached_display_name()
                .map(|dn| dn.to_string())
                .unwrap_or_default();
            let sender_nick = redacted.sender.localpart();
            let datetime = format_ts_millis(u64::from(redacted.origin_server_ts.0));
            let event_id_detail = if context.print_event_id {
                format!(" | {}", redacted.event_id)
            } else {
                String::new()
            };
            println!(
                "Message received for room {} [{}] | sender {} [{}] | {}{} | Received redacted event: sender: {}",
                room_nick, room_id, sender_nick, redacted.sender, datetime, event_id_detail, redacted.sender,
            );
        }
    };
}

/// Data structure needed to pass additional arguments into the event handler
#[derive(Clone, Debug)]
struct EvHandlerContext {
    whoami: OwnedUserId,
    listen_self: bool,
    output: Output,
    print_event_id: bool,
}

/// Listen to all rooms once. Then continue.
pub(crate) async fn listen_once(
    client: &Client,
    listen_self: bool, // listen to my own messages?
    whoami: OwnedUserId,
    output: Output,
    print_event_id: bool,
) -> Result<(), Error> {
    info!(
        "mclient::listen_once(): listen_self {}, room {}",
        listen_self, "all"
    );

    let context = EvHandlerContext {
        whoami,
        listen_self,
        output,
        print_event_id,
    };

    client.add_event_handler_context(context.clone());

    // Todo: print events nicely and filter by --listen-self
    client.add_event_handler(|ev: SyncRoomMessageEvent, room: Room,
        client: Client, context: Ctx<EvHandlerContext>| async move {
        handle_syncroommessageevent(ev, room, client, context).await;
    });

    client.add_event_handler(
        |ev: RedactedSyncRoomMessageEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_redactedsyncroommessageevent(
                ev, room, client, context,
            ).await;
        },
    );

    client.add_event_handler(
        |ev: SyncRoomRedactionEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_syncroomredactedevent(ev, room, client, context).await;
        },
    );

    client.add_event_handler(
        |ev: OriginalSyncRoomEncryptedEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_originalsyncroomencryptedevent(
                ev, room, client, context,
            ).await;
        },
    );

    client.add_event_handler(
        |ev: SyncRoomEncryptedEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_syncroomencryptedevent(ev, room, client, context).await;
        },
    );

    // go into event loop to sync and to execute verify protocol
    info!("Ready and getting messages from server...");

    // get the current sync state from server before syncing
    // This gets all rooms but ignores msgs from itself.
    let settings = SyncSettings::default();

    client.sync_once(settings).await?;
    Ok(())
}

/// Listen to all rooms forever. Stay in the event loop.
pub(crate) async fn listen_forever(
    client: &Client,
    listen_self: bool, // listen to my own messages?
    whoami: OwnedUserId,
    output: Output,
    print_event_id: bool,
) -> Result<(), Error> {
    info!(
        "mclient::listen_forever(): listen_self {}, room {}",
        listen_self, "all"
    );

    let context = EvHandlerContext {
        whoami,
        listen_self,
        output,
        print_event_id,
    };

    client.add_event_handler_context(context.clone());

    // Todo: print events nicely and filter by --listen-self
    client.add_event_handler(
        |ev: SyncRoomMessageEvent, room: Room, client: Client, context: Ctx<EvHandlerContext>| async move {
        handle_syncroommessageevent(ev, room, client, context).await;
    });

    client.add_event_handler(
        |ev: SyncRoomEncryptedEvent, room: Room, client: Client, context: Ctx<EvHandlerContext>| async move {
        handle_syncroomencryptedevent(ev, room, client, context).await;
    });

    client.add_event_handler(
        |ev: OriginalSyncRoomEncryptedEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_originalsyncroomencryptedevent(
                ev, room, client, context,
            ).await;
        },
    );

    client.add_event_handler(
        |ev: RedactedSyncRoomMessageEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_redactedsyncroommessageevent(
                ev, room, client, context,
            ).await;
        },
    );

    client.add_event_handler(|ev: SyncRoomRedactionEvent,
            room: Room, client: Client, context: Ctx<EvHandlerContext>| async move {
            handle_syncroomredactedevent(ev, room, client, context).await;
        });

    // go into event loop to sync and to execute verify protocol
    info!("Ready and waiting for messages ...");
    info!("Once done listening, kill the process manually with Control-C.");

    // get the current sync state from server before syncing
    let settings = SyncSettings::default();

    match client.sync(settings).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // this does not catch Control-C
            error!("Event loop reported: {:?}", e);
            Ok(())
        }
    }
}

#[allow(dead_code)]
fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

/// Get last N messages from some specified rooms once, then go on.
/// Listens to the room(s) specified in the argument, prints the last N messasges.
/// The read messages can be already read ones or new unread ones.
/// Then it returns. Less than N messages might be printed if the messages do not exist.
/// Running it twice in a row (while no new messages were sent) should deliver the same output, response.
pub(crate) async fn listen_tail(
    client: &Client,
    roomnames: &Vec<String>, // roomId
    number: u64,             // number of messages to print, N
    listen_self: bool,       // listen to my own messages?
    whoami: OwnedUserId,
    output: Output,
    print_event_id: bool,
) -> Result<(), Error> {
    info!(
        "mclient::listen_tail(): listen_self {}, roomnames {:?}",
        listen_self, roomnames
    );
    if roomnames.is_empty() {
        return Err(Error::MissingRoom);
    }

    // We are *not* using the event manager, no sync()!

    info!("Ready and getting messages from server ...");

    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for roomname in roomnames {
        roomids.push(match RoomId::parse(roomname.clone()) {
            Ok(id) => id,
            Err(ref e) => {
                error!(
                    "Error: invalid room id {:?}. Error reported is {:?}.",
                    roomname, e
                );
                continue;
            }
        });
    }
    let ownedroomidvecoption: Option<Vec<OwnedRoomId>> = Some(roomids.clone());
    // // old code, when there was only 1 roomname
    // let roomclone = roomnames[0].clone();
    // let ownedroomid: OwnedRoomId = RoomId::parse(&roomclone).unwrap();
    // let ownedroomidvec = ownedroomid
    // let ownedroomidvecoption: Option<Vec<OwnedRoomId>> = Some(ownedroomid);
    let mut filter = FilterDefinition::default();
    let mut roomfilter = RoomFilter::empty();
    roomfilter.rooms = ownedroomidvecoption;
    filter.room = roomfilter;

    // Filter by limit. This works.
    // This gets the last N not yet read messages. If all messages had been read, it gets 0 messages.
    let mut roomtimeline = RoomEventFilter::empty();
    roomtimeline.limit = UInt::new(number);
    filter.room.timeline = roomtimeline;

    // see: https://docs.rs/matrix-sdk/0.6.2/src/matrix_sdk/room/common.rs.html#167-200
    // there is also something like next_batch, a value indicating a point in the event timeline
    // prev_batch: https://docs.rs/matrix-sdk/0.6.2/src/matrix_sdk/room/common.rs.html#1142-1180
    // https://docs.rs/matrix-sdk/0.6.2/matrix_sdk/struct.BaseRoom.html#method.last_prev_batch
    // https://docs.rs/matrix-sdk/0.6.2/matrix_sdk/room/struct.Common.html#method.messages
    // https://docs.rs/matrix-sdk/0.6.2/matrix_sdk/room/struct.MessagesOptions.html

    let context = EvHandlerContext {
        whoami: whoami.clone(),
        listen_self,
        output,
        print_event_id,
    };
    let ctx = Ctx(context);

    for roomid in roomids.iter() {
        let mut options = MessagesOptions::backward(); // .from("t47429-4392820_219380_26003_2265");
        options.limit = UInt::new(number).ok_or(Error::InvalidRoom)?;
        let jroom = client.get_room(roomid.clone().as_ref()).ok_or(Error::InvalidRoom)?;
        let msgs = jroom.messages(options).await;
        // debug!("\n\nmsgs = {:?} \n\n", msgs);
        let chunk = msgs.map_err(|e| {
            error!("Failed to get messages: {}", e);
            Error::ListenFailed
        })?.chunk;
        for index in 0..chunk.len() {
            debug!(
                "processing message {:?} out of {:?}",
                index + 1,
                chunk.len()
            );
            let anytimelineevent = &chunk[chunk.len() - 1 - index]; // reverse ordering, getting older msg first
                                                                    // Todo : dump the JSON serialized string via Json API

            if !output.is_text() {
                // JSON output mode: skip full deserialization, just print raw JSON.
                // Extract only the sender field for self-filtering using a minimal struct
                // instead of parsing the entire JSON into serde_json::Value.
                #[derive(serde::Deserialize)]
                struct SenderOnly {
                    #[serde(default)]
                    sender: Option<String>,
                }
                let raw_json = anytimelineevent.raw().json();
                let is_self = serde_json::from_str::<SenderOnly>(raw_json.get())
                    .ok()
                    .and_then(|s| s.sender)
                    .map(|s| s == whoami.as_str())
                    .unwrap_or(false);
                if listen_self || !is_self {
                    println!("{}", raw_json);
                }
                continue;
            }

            let rawevent: AnySyncTimelineEvent = match anytimelineevent.raw().deserialize() {
                Ok(ev) => ev,
                Err(e) => {
                    error!("Failed to deserialize timeline event: {}", e);
                    continue;
                }
            };
            // print_type_of(&rawevent); // ruma_common::events::enums::AnyTimelineEvent
            debug!("rawevent = value is {:?}\n", rawevent);

            match rawevent {
                AnySyncTimelineEvent::MessageLike(anymessagelikeevent) => {
                    debug!("value: {:?}", anymessagelikeevent);
                    match anymessagelikeevent {
                        AnySyncMessageLikeEvent::RoomMessage(messagelikeevent) => {
                            debug!("value: {:?}", messagelikeevent);
                            match messagelikeevent {
                                SyncMessageLikeEvent::Original(orginialmessagelikeevent) => {
                                    let room_id = roomid.clone();
                                    let room_nick = jroom.cached_display_name()
                                        .map(|dn| dn.to_string())
                                        .unwrap_or_default();
                                    handle_originalsyncmessagelikeevent(
                                        &orginialmessagelikeevent,
                                        &room_id,
                                        &room_nick,
                                        &ctx,
                                    );
                                }
                                SyncMessageLikeEvent::Redacted(redacted) => {
                                    let room_nick = jroom.cached_display_name()
                                        .map(|dn| dn.to_string())
                                        .unwrap_or_default();
                                    let sender_nick = redacted.sender.localpart();
                                    let datetime = format_ts_millis(u64::from(redacted.origin_server_ts.0));
                                    let event_id_detail = if print_event_id {
                                        format!(" | {}", redacted.event_id)
                                    } else {
                                        String::new()
                                    };
                                    println!(
                                        "Message received for room {} [{}] | sender {} [{}] | {}{} | Received redacted event: sender: {}",
                                        room_nick, roomid, sender_nick, redacted.sender, datetime, event_id_detail, redacted.sender,
                                    );
                                }
                            }
                        }
                        AnySyncMessageLikeEvent::RoomEncrypted(messagelikeevent) => {
                            debug!(
                                "Event of type RoomEncrypted received: {:?}",
                                messagelikeevent
                            );
                            // Cannot be decrypted with jroom.decrypt_event() for messages()
                            // (only works for sync events). Print readable output.
                            let sender = messagelikeevent.sender();
                            if whoami != *sender || listen_self {
                                let room_nick = jroom.cached_display_name()
                                    .map(|dn| dn.to_string())
                                    .unwrap_or_default();
                                let sender_nick = sender.localpart();
                                let datetime = format_ts_millis(u64::from(messagelikeevent.origin_server_ts().0));
                                let event_id_detail = if print_event_id {
                                    format!(" | {}", messagelikeevent.event_id())
                                } else {
                                    String::new()
                                };
                                println!(
                                    "Message received for room {} [{}] | sender {} [{}] | {}{} | Encrypted message could not be decrypted",
                                    room_nick, roomid, sender_nick, sender, datetime, event_id_detail,
                                );
                            } else {
                                debug!("Skipping message from itself because --listen-self is not set.");
                            }
                        }
                        AnySyncMessageLikeEvent::RoomRedaction(messagelikeevent) => {
                            let sender = messagelikeevent.sender();
                            if whoami != *sender || listen_self {
                                let room_nick = jroom.cached_display_name()
                                    .map(|dn| dn.to_string())
                                    .unwrap_or_default();
                                let sender_nick = sender.localpart();
                                let datetime = format_ts_millis(u64::from(messagelikeevent.origin_server_ts().0));
                                let event_id_detail = if print_event_id {
                                    format!(" | {}", messagelikeevent.event_id())
                                } else {
                                    String::new()
                                };
                                println!(
                                    "Message received for room {} [{}] | sender {} [{}] | {}{} | Received redaction event: sender: {}",
                                    room_nick, roomid, sender_nick, sender, datetime, event_id_detail, sender,
                                );
                            }
                        }
                        _ => {
                            // Other MessageLike events (reactions, stickers, etc.)
                            debug!("Received other MessageLike event type, skipping: {:?}", anymessagelikeevent);
                        }
                    }
                }
                _ => debug!("State event, not interested in that."),
            }
        }
    }
    Ok(())
}

/// Listen to some specified rooms once, then go on.
/// Listens to the room(s) provided as argument, prints any pending relevant messages,
/// and then continues by returning.
pub(crate) async fn listen_all(
    client: &Client,
    roomnames: &Vec<String>, // roomId
    listen_self: bool,       // listen to my own messages?
    whoami: OwnedUserId,
    output: Output,
    print_event_id: bool,
) -> Result<(), Error> {
    if roomnames.is_empty() {
        return Err(Error::MissingRoom);
    }
    info!(
        "mclient::listen_all(): listen_self {}, roomnames {:?}",
        listen_self, roomnames
    );

    let context = EvHandlerContext {
        whoami,
        listen_self,
        output,
        print_event_id,
    };

    client.add_event_handler_context(context.clone());

    // Todo: print events nicely and filter by --listen-self
    client.add_event_handler(|ev: SyncRoomMessageEvent, room: Room, client: Client, context: Ctx<EvHandlerContext>| async move {
        handle_syncroommessageevent(ev, room, client, context).await;
    });

    // this seems idential to SyncRoomMessageEvent and hence a duplicate
    // client.add_event_handler(
    //     |ev: OriginalSyncRoomMessageEvent, _client: Client| async move {
    //         println!(
    //             "Received a message for OriginalSyncRoomMessageEvent {:?}",
    //             ev
    //         );
    //     },
    // );

    client.add_event_handler(
        |ev: RedactedSyncRoomMessageEvent,
         room: Room,
         client: Client,
         context: Ctx<EvHandlerContext>| async move {
            handle_redactedsyncroommessageevent(
                ev, room, client, context,
            ).await;
        },
    );

    client.add_event_handler(|ev: SyncRoomRedactionEvent,
        room: Room, client: Client, context: Ctx<EvHandlerContext>| async move {
        handle_syncroomredactedevent(ev, room, client, context).await;
    });

    // go into event loop to sync and to execute verify protocol
    info!("Ready and waiting for messages ...");

    // search for filter: https://docs.rs/matrix-sdk/0.6.2/matrix_sdk/struct.Client.html#method.builder
    // https://docs.rs/ruma/latest/ruma/api/client/filter/struct.FilterDefinition.html
    // https://docs.rs/ruma/latest/ruma/api/client/filter/struct.RoomFilter.html  ==> timeline, rooms
    // https://docs.rs/ruma/latest/ruma/api/client/filter/struct.RoomEventFilter.html  ==> limit : max number of events to return, types :: event types to include; rooms: rooms to include

    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for roomname in roomnames {
        roomids.push(RoomId::parse(roomname.clone()).unwrap());
    }
    let ownedroomidvecoption: Option<Vec<OwnedRoomId>> = Some(roomids);
    // Filter by rooms. This works.
    let mut filter = FilterDefinition::default();
    let mut roomfilter = RoomFilter::empty();
    roomfilter.rooms = ownedroomidvecoption;
    filter.room = roomfilter;

    // // Let's enable member lazy loading. This filter is enabled by default.
    // filter.room.state.lazy_load_options = LazyLoadOptions::Enabled {
    //     include_redundant_members: false,
    // };

    // // Todo: add future option like --user to filter messages by user id
    // // Filter by user; this works but NOT for itself.
    // // It does not listen to its own messages, additing itself as sender does not help.
    // // The msgs sent by itself are not in this event stream and hence cannot be filtered.
    // let mut roomstate = RoomEventFilter::empty();
    // let userid1: OwnedUserId = UserId::parse("@john:some.homeserver.org").unwrap();
    // let userid2: OwnedUserId = UserId::parse("@jane:some.homeserver.org").unwrap();
    // let useridslice = &[userid1, userid2][0..2];
    // roomstate.senders = Some(useridslice);
    // filter.room.timeline = roomstate;

    // // Filter by limit. This works.
    // // This gets the last N not yet read messages. If all messages had been read, it gets 0 messages.
    // let mut roomtimeline = RoomEventFilter::empty();
    // roomtimeline.limit = UInt::new(number);
    // filter.room.timeline = roomtimeline;

    // To be more efficient, more performant, usually one stores the filter on the
    // server under a given name. This way only the name but not the filter needs to
    // be transferred. But we would have an unlimited amount of filters. How to name them
    // uniquely? To avoid the naming problem, we do not create names but send the filter
    // itself.
    // The filter would be created like so:
    // // some unique naming scheme, dumb example prepending the room id with "room-",
    // // some sort of hash would be better.
    // let filter_name = format!("room-{}", room);
    // let filter_id = client
    //     .get_or_upload_filter(&filter_name, filter)
    //     .await
    //     .unwrap();
    // // now we can use the filter_name in the sync() call
    // let sync_settings = SyncSettings::new().filter(Filter::FilterId(&filter_id));

    // let sync_settings = SyncSettings::default()
    //     .token(client.sync_token().await.unwrap())
    //     .filter(Filter::FilterId(&filter_id));

    let mut err_count = 0u32;
    let filterclone = filter.clone();
    let sync_settings = SyncSettings::default().filter(Filter::FilterDefinition(filterclone));

    match client.sync_once(sync_settings).await {
        Ok(response) => debug!("listen_all successful {:?}", response),
        Err(ref e) => {
            err_count += 1;
            error!("listen_all returned error {:?}", e);
        }
    }
    if err_count != 0 {
        Err(Error::ListenFailed)
    } else {
        Ok(())
    }
}
