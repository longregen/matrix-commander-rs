//
// https://www.github.com/8go/matrix-commander-rs
// mclient.rs
//

//! Module that bundles code together that uses the `matrix-sdk` API.
//! Primarily the matrix_sdk::Client API
//! (see <https://docs.rs/matrix-sdk/latest/matrix_sdk/struct.Client.html>).
//! This module implements the matrix-sdk-based portions of the primitives like
//! logging in, logging out, verifying, sending messages, sending files, etc.
//! It excludes receiving and listening (see listen.rs).

use chrono::{DateTime, Local};
use mime::Mime;
use std::borrow::Cow;
use std::io::{self, Read, Write};
// use std::env;
use std::fs;
use std::fs::File;
// use std::ops::Deref;
// use std::path::Path;
use std::io::{stdin, IsTerminal};
use std::path::PathBuf;
use tracing::{debug, error, info, warn};
// use thiserror::Error;
// use directories::ProjectDirs;
// use serde::{Deserialize, Serialize};
//use serde_json::Result;
use url::Url;

use matrix_sdk::{
    attachment::AttachmentConfig,
    authentication::{matrix::MatrixSession, SessionTokens},
    config::{RequestConfig, StoreConfig, SyncSettings},
    media::{MediaFormat, MediaRequestParameters},
    room,
    room::{Room, RoomMember},
    ruma::{
        api::client::room::create_room::v3::Request as CreateRoomRequest,
        api::client::room::create_room::v3::RoomPreset,
        api::client::room::Visibility,
        api::client::uiaa,
        events::room::encryption::RoomEncryptionEventContent,
        events::room::member::RoomMemberEventContent,
        events::room::message::{
            EmoteMessageEventContent,
            MessageType,
            NoticeMessageEventContent,
            RoomMessageEventContent,
            TextMessageEventContent,
        },
        events::room::name::RoomNameEventContent,
        events::room::power_levels::RoomPowerLevelsEventContent,
        events::room::topic::RoomTopicEventContent,
        events::room::MediaSource,
        events::AnyInitialStateEvent,
        events::EmptyStateKey,
        events::InitialStateEvent,
        serde::Raw,
        EventEncryptionAlgorithm,
        OwnedDeviceId,
        OwnedMxcUri,
        OwnedRoomAliasId,
        OwnedRoomId,
        OwnedUserId,
        RoomAliasId,
        RoomId,
        UserId,
    },
    Client,
    EncryptionState,
    RoomMemberships,
    SessionMeta,
};

use matrix_sdk_base::RoomStateFilter;

use std::time::Duration;

use crate::args::Args;
use crate::cli::get_password;
use crate::types::*;
use crate::{get_store_default_path, get_store_depreciated_default_path};

// import verification code
#[path = "emoji_verify.rs"]
mod emoji_verify;

/// Convert String to Option with '' being converted to None
fn to_opt(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Replace '*' in room id list with all rooms the user knows about (joined, left, invited, etc.)
pub(crate) fn replace_star_with_rooms(client: &Client, vecstr: &mut Vec<String>) {
    let blen = vecstr.len();
    vecstr.retain(|x| x.trim() != "*");
    let alen = vecstr.len();
    if blen == alen {
        return;
    }
    for r in client.rooms() {
        vecstr.push(r.room_id().to_string());
    }
}

/// Convert partial room id, partial room alias, room alias to
/// full room id.
/// !irywieryiwre => !irywieryiwre:matrix.server.com
/// john => !irywieryiwre:matrix.server.com
/// #john => !irywieryiwre:matrix.server.com
/// #john:matrix.server.com => !irywieryiwre:matrix.server.com
pub(crate) async fn convert_to_full_room_id(
    client: &Client,
    room: &mut String,
    default_host: &str,
) {
    room.retain(|c| !c.is_whitespace());
    if room.starts_with('@') {
        error!(
            "This room alias or id {:?} starts with an at sign. \
            @ are used for user ids, not room id or room aliases. \
            This will fail later.",
            room
        );
        return;
    }
    if !room.starts_with('#') && !room.starts_with('!') {
        room.insert(0, '#');
    }
    if !room.contains(':') {
        room.push(':');
        room.push_str(default_host);
    }
    // now we either full room id or full room alias id
    if room.starts_with('!') {
        return;
    }
    if room.starts_with("\\!") {
        room.remove(0); // remove escape
        return;
    }
    if room.starts_with("\\#") {
        room.remove(0); // remove escape
        return;
    }

    if room.starts_with('#') {
        match RoomAliasId::parse(room.replace("\\#", "#")) {
            //remove possible escape
            Ok(id) => match client.resolve_room_alias(&id).await {
                Ok(res) => {
                    room.clear();
                    room.push_str(res.room_id.as_ref());
                }
                Err(ref e) => {
                    error!(
                        "Error: invalid alias {:?}. resolve_room_alias() returned error {:?}.",
                        room, e
                    );
                    room.clear();
                }
            },
            Err(ref e) => {
                error!(
                    "Error: invalid alias {:?}. Error reported is {:?}.",
                    room, e
                );
                room.clear();
            }
        }
    }
}

/// Convert partial room ids, partial room aliases, room aliases to
/// full room ids.
/// !irywieryiwre => !irywieryiwre:matrix.server.com
/// john => !irywieryiwre:matrix.server.com
/// #john => !irywieryiwre:matrix.server.com
/// #john:matrix.server.com => !irywieryiwre:matrix.server.com
pub(crate) async fn convert_to_full_room_ids(
    client: &Client,
    vecstr: &mut Vec<String>,
    default_host: &str,
) {
    vecstr.retain(|x| !x.trim().is_empty());
    let num = vecstr.len();
    let mut i = 0;
    while i < num {
        convert_to_full_room_id(client, &mut vecstr[i], default_host).await;
        i += 1;
    }
    vecstr.retain(|x| !x.trim().is_empty());
}

/// Convert partial mxc uris to full mxc uris.
/// SomeStrangeUriKey => "mxc://matrix.server.org/SomeStrangeUriKey"
/// Default_host is a string like "matrix.server.org" or "127.0.0.1"
pub(crate) async fn convert_to_full_mxc_uris(vecstr: &mut Vec<OwnedMxcUri>, default_host: &str) {
    vecstr.retain(|x| !x.as_str().trim().is_empty());
    let num = vecstr.len();
    let mut i = 0;
    while i < num {
        let mut s = vecstr[i].as_str().to_string();
        s.retain(|c| !c.is_whitespace());
        if s.is_empty() {
            debug!("Skipping {:?} because it is empty.", vecstr[i]);
            vecstr[i] = OwnedMxcUri::from("");
            i += 1;
            continue;
        }
        if s.starts_with("mxc://") {
            debug!("Skipping {:?}.", vecstr[i]);
            i += 1;
            continue;
        }
        if s.contains(':') || s.contains('/') {
            error!(
                "This does not seem to be a short MXC URI. Contains : or /. \
                Skipping {:?}. This will likely cause a failure later.",
                vecstr[i]
            );
            i += 1;
            continue;
        }
        let mxc = "mxc://".to_owned() + default_host + "/" + s.as_str();
        vecstr[i] = OwnedMxcUri::from(mxc);
        if !vecstr[i].is_valid() {
            error!(
                "This does not seem to be a short MXC URI. Contains : or /. \
                Skipping {:?}. This will likely cause a failure later.",
                vecstr[i]
            );
        }
        i += 1;
    }
    vecstr.retain(|x| !x.as_str().trim().is_empty());
}

/// Convert partial user ids to full user ids.
/// john => @john:matrix.server.com
/// @john => @john:matrix.server.com
/// @john:matrix.server.com => @john:matrix.server.com
pub(crate) fn convert_to_full_user_ids(vecstr: &mut Vec<String>, default_host: &str) {
    vecstr.retain(|x| !x.trim().is_empty());
    for el in vecstr {
        el.retain(|c| !c.is_whitespace());
        if el.starts_with('!') {
            error!(
                "This user id {:?} starts with an exclamation mark. \
                ! are used for rooms, not users. This will fail later.",
                el
            );
            continue;
        }
        if el.starts_with('#') {
            error!(
                "This user id {:?} starts with a hash tag.
            # are used for room aliases, not users. This will fail later.",
                el
            );
            continue;
        }
        if !el.starts_with('@') {
            el.insert(0, '@');
        }
        if !el.contains(':') {
            el.push(':');
            el.push_str(default_host);
        }
    }
}

/// Convert partial room alias ids to full room alias ids.
/// john => #john:matrix.server.com
/// #john => #john:matrix.server.com
/// #john:matrix.server.com => #john:matrix.server.com
pub(crate) fn convert_to_full_alias_ids(vecstr: &mut Vec<String>, default_host: &str) {
    vecstr.retain(|x| !x.trim().is_empty());
    for el in vecstr {
        el.retain(|c| !c.is_whitespace());
        if el.starts_with('!') {
            warn!(
                "This room alias {:?} starts with an exclamation mark. \
                ! are used for rooms ids, not aliases. This might cause problems later.",
                el
            );
            continue;
        }
        if el.starts_with('@') {
            error!(
                "This room alias {:?} starts with an at sign. \
                @ are used for user ids, not aliases. This will fail later.",
                el
            );
            continue;
        }
        if !el.starts_with('#') {
            el.insert(0, '#');
        }
        if !el.contains(':') {
            el.push(':');
            el.push_str(default_host);
        }
    }
}

/// Convert full room alias ids to local canonical short room alias ids.
/// #john:matrix.server.com => john
/// #john => john
/// john => john
/// Does NOT remove empty items from vector.
pub(crate) fn convert_to_short_canonical_alias_ids(vecstr: &mut Vec<String>) {
    // don't remove empty ones: vecstr.retain(|x| !x.trim().is_empty());
    // keep '' so we can set the alias to null, e.g. in room_create()
    for el in vecstr {
        el.retain(|c| !c.is_whitespace());
        if el.starts_with('!') {
            warn!(
                "This room alias {:?} starts with an exclamation mark. \
                ! are used for rooms ids, not aliases. This might cause problems later.",
                el
            );
            continue;
        }
        if el.starts_with('#') {
            el.remove(0);
        }
        if el.contains(':') {
            match el.find(':') {
                None => (),
                Some(i) => el.truncate(i),
            }
        }
    }
}

/// Constructor for Credentials.
pub(crate) fn restore_credentials(ap: &Args) -> Result<Credentials, Error> {
    if ap.credentials.is_file() {
        let credentials = Credentials::load(&ap.credentials)?;
        let mut credentialsfiltered = credentials.clone();
        credentialsfiltered.access_token = "***".to_string();
        debug!(
            "restore_credentials: loaded credentials are: {:?}",
            credentialsfiltered
        );
        Ok(credentials)
    } else {
        Err(Error::NoCredentialsFound)
    }
}

/// Constructor for matrix-sdk async Client, based on restore_login().
pub(crate) async fn restore_login(credentials: &Credentials, ap: &Args, needs_sync: bool) -> Result<Client, Error> {
    let clihomeserver = ap.homeserver.clone();
    let homeserver = clihomeserver.unwrap_or_else(|| credentials.homeserver.clone());
    info!(
        "restoring device with device_id = {:?} on homeserver {:?}.",
        &credentials.device_id, &homeserver
    );

    // let session: matrix_sdk::SessionMeta = credentials.clone().into();
    let client = create_client(&homeserver, ap).await?;

    // let auth = client.matrix_auth();
    // debug!("Called matrix_auth()");
    // debug!("matrix_auth() successful");

    let msession = MatrixSession {
        meta: SessionMeta {
            user_id: credentials.user_id.clone(),
            device_id: credentials.device_id.clone(),
        },
        tokens: SessionTokens {
            access_token: credentials.access_token.clone(),
            refresh_token: None,
        },
    };

    let user_id = msession.meta.user_id.to_string();
    let device_id = msession.meta.device_id.to_string();
    let access_token = msession.tokens.access_token.clone();
    let res = client.restore_session(msession).await;
    match res {
        Ok(_) => {
            debug!("restore_session() successful.");
            debug!(
                "Logged in as {}, got device_id {} and access_token {}",
                user_id, device_id, redact_token(&access_token),
            );
        }
        Err(e) => {
            error!(
                "Error: Login failed because restore_session() failed. \
                Error: {}",
                e
            );
            return Err(Error::LoginFailed);
        }
    }

    debug!("restore_login returned successfully. Logged in now.");
    if ap.listen == Listen::Never {
        if needs_sync {
            sync_once(&client, ap.timeout, ap.sync).await?;
        } else {
            info!("Skipping sync: no commands require room state.");
        }
    } else {
        info!("Skipping sync due to --listen");
    }
    Ok(client)
}

/// Constructor for matrix-sdk async Client, based on login_username().
pub(crate) async fn login<'a>(
    ap: &'a mut Args,
    homeserver: &Url,
    username: &str,
    password: &str,
    device: &str,
    room_default: &str,
) -> Result<(Client, Credentials), Error> {
    let client = create_client(homeserver, ap).await?;
    debug!("About to call login_username()");
    // we need to log in.
    let response = client
        .matrix_auth()
        .login_username(username, password)
        .initial_device_display_name(device)
        .send()
        .await;
    debug!("Called login_username()");

    match response {
        Ok(_n) => debug!("login_username() successful."),
        Err(e) => {
            error!("Error: {}", e);
            return Err(Error::LoginFailed);
        }
    }
    let _ = client
        .session()
        .expect("Error: client not logged in correctly. No session.");
    info!("device id = {}", client.session_meta().unwrap().device_id);
    info!("credentials file = {:?}", ap.credentials);

    let credentials = Credentials::new(
        homeserver.clone(),
        client.session_meta().unwrap().user_id.clone(),
        client.access_token().unwrap(),
        client.session_meta().unwrap().device_id.clone(),
        room_default.to_string(),
        client.session_tokens().and_then(|t| t.refresh_token.clone()),
    );
    credentials.save(&ap.credentials)?;
    // sync is needed even when --login is used,
    // because after --login argument, arguments like -m or --rooms might
    // be used, e.g. in the login-fire-off-a-msg-and-forget scenario
    if ap.listen == Listen::Never {
        sync_once(&client, ap.timeout, ap.sync).await?;
    } else {
        info!("Skipping sync due to --listen");
    }
    Ok((client, credentials))
}

/// Constructor for matrix-sdk async Client, based on an explicit access token.
/// This is used when --login access-token is specified along with --access-token.
pub(crate) async fn login_access_token<'a>(
    ap: &'a mut Args,
    homeserver: &Url,
    username: &str,
    access_token: &str,
    device: &str,
    room_default: &str,
) -> Result<(Client, Credentials), Error> {
    let client = create_client(homeserver, ap).await?;
    debug!("About to restore session with explicit access token");

    let user_id = match UserId::parse(username) {
        Ok(u) => u,
        Err(e) => {
            error!("Error: Invalid user id {:?}: {}", username, e);
            return Err(Error::LoginFailed);
        }
    };

    // Generate a device id from the device name if not available
    let device_id = OwnedDeviceId::from(device);

    let msession = MatrixSession {
        meta: SessionMeta {
            user_id: user_id.clone(),
            device_id: device_id.clone(),
        },
        tokens: SessionTokens {
            access_token: access_token.to_string(),
            refresh_token: None,
        },
    };

    match client.restore_session(msession).await {
        Ok(_) => {
            debug!("restore_session() with access token successful.");
            info!(
                "Logged in as {} with device_id {}",
                user_id, device_id
            );
        }
        Err(e) => {
            error!(
                "Error: Login with access token failed because restore_session() failed. \
                Error: {}",
                e
            );
            return Err(Error::LoginFailed);
        }
    }

    let credentials = Credentials::new(
        homeserver.clone(),
        user_id,
        access_token.to_string(),
        device_id,
        room_default.to_string(),
        None,
    );
    credentials.save(&ap.credentials)?;

    if ap.listen == Listen::Never {
        sync_once(&client, ap.timeout, ap.sync).await?;
    } else {
        info!("Skipping sync due to --listen");
    }
    Ok((client, credentials))
}

/// Constructor for matrix-sdk async Client, based on SSO login.
/// This is used when --login sso is specified.
/// It uses the matrix-sdk built-in login_sso() which:
/// 1. Spawns a local HTTP server
/// 2. Calls the provided callback with the SSO URL (we open a browser)
/// 3. Waits for the SSO provider to redirect back with a loginToken
/// 4. Logs in with the token
pub(crate) async fn login_sso<'a>(
    ap: &'a mut Args,
    homeserver: &Url,
    device: &str,
    room_default: &str,
) -> Result<(Client, Credentials), Error> {
    let client = create_client(homeserver, ap).await?;
    debug!("About to call login_sso()");

    let response = client
        .matrix_auth()
        .login_sso(|sso_url| async move {
            debug!("Launching browser to complete SSO login.");
            debug!("SSO URL: {}", &sso_url[..sso_url.len().min(60)]);
            // Try to open browser, matching Python's approach
            let result = std::process::Command::new("xdg-open")
                .arg(&sso_url)
                .spawn();
            match result {
                Ok(_) => {
                    debug!("Browser launched for SSO login.");
                    Ok(())
                }
                Err(_) => {
                    // Fallback: print URL for manual opening
                    eprintln!(
                        "Could not launch browser. Please open this URL manually:\n{}",
                        sso_url
                    );
                    Ok(())
                }
            }
        })
        .initial_device_display_name(device)
        .await;

    debug!("Called login_sso()");

    match response {
        Ok(_n) => debug!("login_sso() successful."),
        Err(e) => {
            error!("Error: SSO login failed: {}", e);
            return Err(Error::LoginFailed);
        }
    }

    let _ = client
        .session()
        .expect("Error: client not logged in correctly. No session.");
    info!("device id = {}", client.session_meta().unwrap().device_id);
    info!("credentials file = {:?}", ap.credentials);

    let credentials = Credentials::new(
        homeserver.clone(),
        client.session_meta().unwrap().user_id.clone(),
        client.access_token().unwrap(),
        client.session_meta().unwrap().device_id.clone(),
        room_default.to_string(),
        client.session_tokens().and_then(|t| t.refresh_token.clone()),
    );
    credentials.save(&ap.credentials)?;

    if ap.listen == Listen::Never {
        sync_once(&client, ap.timeout, ap.sync).await?;
    } else {
        info!("Skipping sync due to --listen");
    }
    Ok((client, credentials))
}

/// Prepares a client that can then be used for actual login.
/// Configures the matrix-sdk async Client.
async fn create_client(homeserver: &Url, ap: &Args) -> Result<Client, Error> {
    // The location to save files to
    let sqlitestorehome = &ap.store;
    // remove in version 0.5 : todo
    // Incompatibility between v0.4 and v0.3-
    debug!(
        "Compare store names: {:?} {:?}",
        ap.store,
        get_store_default_path()
    );
    if ap.store == get_store_default_path()
        && !get_store_default_path().exists()
        && get_store_depreciated_default_path().exists()
    {
        warn!(
            "In order to correct incompatibility in version v0.4 the \
            directory {:?} will be renamed to {:?}.",
            get_store_depreciated_default_path(),
            get_store_default_path()
        );
        fs::rename(
            get_store_depreciated_default_path(),
            get_store_default_path(),
        )?;
    }
    info!("Using sqlite store {:?}", &sqlitestorehome);
    // let builder = if let Some(proxy) = cli.proxy { builder.proxy(proxy) } else { builder };
    let mut builder = Client::builder()
        .homeserver_url(homeserver)
        .store_config(StoreConfig::new("matrix-commander-lock".to_owned()))
        .request_config(
            RequestConfig::new()
                .timeout(Duration::from_secs(ap.timeout)),
        );
    if let Some(ref proxy_url) = ap.proxy {
        info!("Using proxy: {}", proxy_url);
        builder = builder.proxy(proxy_url);
    }
    if ap.no_ssl {
        info!("SSL verification is disabled via --no-ssl.");
        builder = builder.disable_ssl_verification();
    }
    if let Some(ref cert_path) = ap.ssl_certificate {
        info!("Using custom SSL certificate from {:?}.", cert_path);
        let cert_pem = fs::read(cert_path).map_err(|e| {
            error!(
                "Error: Failed to read SSL certificate file {:?}: {}",
                cert_path, e
            );
            Error::InvalidFile
        })?;
        let cert = reqwest::Certificate::from_pem(&cert_pem).map_err(|e| {
            error!(
                "Error: Failed to parse PEM certificate from {:?}: {}",
                cert_path, e
            );
            Error::InvalidFile
        })?;
        builder = builder.add_root_certificates(vec![cert]);
    }
    let client = builder
        .sqlite_store(sqlitestorehome, None)
        .build()
        .await
        .expect("Error: ClientBuilder build failed. Error: cannot add store to ClientBuilder."); // no password for store!
    Ok(client)
}

/// Does bootstrap cross signing
pub(crate) async fn bootstrap(client: &Client, ap: &mut Args) -> Result<(), Error> {
    let userid = &ap.creds.as_ref().unwrap().user_id.clone();
    get_password(ap);
    if let Some(password) = &ap.password {
        let mut css = client.encryption().cross_signing_status().await;
        debug!("Client cross signing status before: {:?}", css);

        if let Err(e) = client.encryption().bootstrap_cross_signing(None).await {
            if let Some(response) = e.as_uiaa_response() {
                let mut password = uiaa::Password::new(
                    uiaa::UserIdentifier::UserIdOrLocalpart(userid.to_string()),
                    password.to_owned(),
                );
                password.session = response.session.clone();

                // Note, on the failed attempt we can use `bootstrap_cross_signing` immediately, to
                // avoid checks.
                debug!("Called bootstrap cross signing {:?}", password.session);
                client
                    .encryption()
                    .bootstrap_cross_signing(Some(uiaa::AuthData::Password(password)))
                    .await
                    .expect("Error: Couldn't bootstrap cross signing.")
            } else {
                error!("Error: {:?}", e);
                return Err(Error::BootstrapFailed);
            }
        }
        css = client.encryption().cross_signing_status().await;
        debug!(
            "bootstrap_cross_signing() was either successful or the cross signing keys were \
            already available in which case nothing is done and password was ignored."
        );
        debug!("Client cross signing status after bootstrapping: {:?}", css);
        Ok(())
    } else {
        Err(Error::MissingPassword)
    }
}

/// Does verification
pub(crate) async fn verify(client: &Client, ap: &Args) -> Result<(), Error> {
    let userid = &ap.creds.as_ref().unwrap().user_id.clone();
    let deviceid = &ap.creds.as_ref().unwrap().device_id.clone();
    debug!("Client logged in: {}", client.is_active());
    debug!("Client user id: {}", userid);
    debug!("Client device id: {}", deviceid);
    debug!(
        "Client access token used: {:?}",
        obfuscate(&client.access_token().unwrap(), 4)
    );

    let css = client.encryption().cross_signing_status().await;
    debug!("Client cross signing status {:?}", css);
    if let Some(cssc) = css {
        if !cssc.has_self_signing {
            warn!(
                "Client cross signing status is false. Verify is likely to fail. \
                Try running --bootstrap first. {:?}",
                cssc
            );
        }
    }

    if ap.verify.is_manual_user() {
        debug!("Will attempt to verify users '{:?}'.", ap.user);
        let mut errcount = 0;
        for userid in &ap.user {
            match UserId::parse(userid.clone()) {
                Ok(uid) => match client.encryption().get_user_identity(&uid).await {
                    Ok(user) => {
                        if let Some(user) = user {
                            match user.verify().await {
                                Ok(()) => {
                                    info!(
                                        "Successfully verified user {:?} in one direction.",
                                        userid
                                    )
                                }
                                Err(e) => {
                                    error!(
                                        "Error: verify failed. Are you logged in? User exists? \
                                        Do you have cross-signing keys available? {:?} {:?}",
                                        userid, e
                                    );
                                    errcount += 1;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error: {:?}", e);
                        errcount += 1;
                    }
                },
                Err(e) => {
                    error!("Error: invalid user id {:?}, Error: {:?}", userid, e);
                    errcount += 1;
                }
            }
        }
        if errcount > 0 {
            return Err(Error::VerifyFailed);
        }
    } else if ap.verify.is_manual_device() {
        let response = client.devices().await?;
        for device in response.devices {
            let deviceid = device.device_id;

            match client.encryption().get_device(userid, &deviceid).await {
                Ok(device) => {
                    if let Some(device) = device {
                        match device.verify().await {
                            Ok(()) => info!(
                                "Successfully verified device {:?} in one direction.",
                                deviceid
                            ),
                            Err(e) => {
                                error!(
                                    "Error: verify failed. Are you logged in? Device is yours? \
                                    Do you have cross-signing keys available? {:?} {:?}",
                                    deviceid, e
                                );
                                return Err(Error::VerifyFailed);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error: {:?}", e);
                    return Err(Error::VerifyFailed);
                }
            }
        }
    } else if ap.verify.is_emoji() {
        emoji_verify::sync_wait_for_verification_request(client).await?; // wait in sync for other party to initiate emoji verify
    } else if ap.verify.is_emoji_req() {
        if ap.user.len() != 1 {
            error!(
                "Error: for requesting verification exactly 1 user must be specified with --user. Found {:?}.",
                ap.user
            )
        } else {
            match &ap.device {
                None => error!(
                    "Error: for requesting verification exactly 1 device must be specified with --device. Found {:?}.",
                    ap.device
                ),
                Some(device) => {
                    emoji_verify::sync_request_verification(client, ap.user[0].to_string(), device.to_string()).await?;
                    // request verification from other device
                }
            }
        }
    } else {
        error!(
            "Error: {:?}",
            Error::UnsupportedCliParameter("Option used for --verify is not supported.")
        );
        return Err(Error::VerifyFailed);
    }
    Ok(())
}

/// Logs out, destroying the device and removing credentials file
pub(crate) async fn logout(client: &Client, ap: &Args) -> Result<(), Error> {
    debug!("Logout on client");
    logout_server(client, ap).await?;
    logout_local(ap)
}

/// Logs out locally by removing the credentials file from disk.
pub(crate) fn logout_local(ap: &Args) -> Result<(), Error> {
    if ap.credentials.is_file() {
        match std::fs::remove_file(&ap.credentials) {
            Ok(_) => info!(
                "Local logout: removed credentials file {:?}.",
                &ap.credentials
            ),
            Err(e) => error!(
                "Local logout: failed to remove credentials file {:?}: {}",
                &ap.credentials, e
            ),
        }
    } else {
        debug!(
            "Local logout: credentials file {:?} does not exist, nothing to remove.",
            &ap.credentials
        );
    }
    Ok(())
}

/// Only logs out from server, no local changes.
pub(crate) async fn logout_server(client: &Client, ap: &Args) -> Result<(), Error> {
    if ap.logout.is_me() {
        match client.matrix_auth().logout().await {
            Ok(n) => info!("Logout sent to server {:?}", n),
            Err(e) => error!(
                "Error: Server logout failed but we remove local device id anyway. {:?}",
                e
            ),
        }
    }
    if ap.logout.is_all() {
        debug!(
            "Did nothing on server side. \
            All devices should have been deleted already. \
            Check the log a few lines up."
        );
    }
    Ok(())
}

// Todo: when is this sync() really necessary? send seems to work without,
// listen do not need it, devices does not need it but forces it to consume msgs, ...
/// Utility function to synchronize once.
pub(crate) async fn sync_once(client: &Client, timeout: u64, stype: Sync) -> Result<(), Error> {
    debug!("value of sync in sync_once() is {:?}", stype);
    if stype.is_off() {
        info!("syncing is turned off. No syncing.");
    }
    if stype.is_full() {
        let effective_timeout = std::cmp::min(timeout, 30);
        info!("syncing once, timeout set to {} seconds ...", effective_timeout);
        client
            .sync_once(SyncSettings::new().timeout(Duration::new(effective_timeout, 0)).full_state(true))
            .await?;
        info!("sync completed");
    }
    Ok(())
}

/*pub(crate) fn room(&self, room_id: &RoomId) -> Result<room::Room> {
    self.get_room(room_id).ok_or(Error::InvalidRoom)
}*/

/*pub(crate) fn invited_room(&self, room_id: &RoomId) -> Result<room::Invited> {
    self.get_invited_room(room_id).ok_or(Error::InvalidRoom)
}*/

// pub(crate) fn joined_room(client: Client, room_id: &RoomId) -> Result<room::Joined> {
//     client.get_joined_room(room_id).ok_or(Error::InvalidRoom)
// }

/*pub(crate) fn left_room(&self, room_id: &RoomId) -> Result<room::Left> {
    self.get_left_room(room_id).ok_or(Error::InvalidRoom)
}*/

/// Print list of devices of the current user.
pub(crate) async fn devices(client: &Client, output: Output, sep: &str) -> Result<(), Error> {
    debug!("Devices on server");
    let response = client.devices().await?;
    for device in response.devices {
        match output {
            Output::Text => {
                // Match Python format: device_id    display_name    last_seen_ip    last_seen_date
                let display_name = device.display_name.as_deref().unwrap_or("");
                let last_seen_ip = device.last_seen_ip.as_deref().unwrap_or("");
                let last_seen_ts = device.last_seen_ts
                    .map(|ts| {
                        let ms = u64::from(ts.0);
                        let secs = (ms / 1000) as i64;
                        let nsecs = ((ms % 1000) * 1_000_000) as u32;
                        DateTime::from_timestamp(secs, nsecs)
                            .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| ms.to_string())
                    })
                    .unwrap_or_default();
                println!(
                    "{}{}{}{}{}{}{}",
                    device.device_id, sep,
                    display_name, sep,
                    last_seen_ip, sep,
                    last_seen_ts,
                );
            }
            Output::JsonSpec => (),
            _ => {
                let last_seen_ts = device.last_seen_ts
                    .map(|ts| {
                        let ms = u64::from(ts.0);
                        let secs = (ms / 1000) as i64;
                        let nsecs = ((ms % 1000) * 1_000_000) as u32;
                        DateTime::from_timestamp(secs, nsecs)
                            .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| ms.to_string())
                    })
                    .unwrap_or_default();
                print_json(
                    &serde_json::json!({
                        "device_id": device.device_id.to_string(),
                        "display_name": device.display_name.as_deref().unwrap_or(""),
                        "last_seen_ip": device.last_seen_ip.as_deref().unwrap_or(""),
                        "last_seen_ts": last_seen_ts.as_str(),
                    }),
                    output,
                    false,
                );
            }
        }
    }
    Ok(())
}

/// Write the avatar of the current user to a file.
pub(crate) async fn get_avatar(
    client: &Client,
    path: &PathBuf,
    _output: Output,
) -> Result<(), Error> {
    debug!("Get avatar from server");
    if let Ok(Some(avatar)) = client.account().get_avatar(MediaFormat::File).await {
        match std::fs::write(path, avatar) {
            Ok(_) => {
                debug!("Avatar saved successfully");
                Ok(())
            }
            Err(e) => Err(Error::IO(e)),
        }
    } else {
        Err(Error::GetAvatarFailed)
    }
}

/// Get the avatar MXC URI of the current user.
pub(crate) async fn get_avatar_url(client: &Client, output: Output, sep: &str) -> Result<(), Error> {
    debug!("Get avatar MXC from server");
    if let Ok(Some(mxc_uri)) = client.account().get_avatar_url().await {
        debug!(
            "Avatar MXC URI obtained successfully. MXC_URI is {:?}",
            mxc_uri
        );
        // Convert MXC URI to HTTP URL (like Python's mxc_to_http)
        let avatar_http = if let Ok((server_name, media_id)) = mxc_uri.parts() {
            let hs = client.homeserver().to_string();
            let proto = hs.split("://").next().unwrap_or("https");
            format!(
                "{}://{}/_matrix/media/r0/download/{}/{}",
                proto, server_name, server_name, media_id
            )
        } else {
            String::new()
        };
        match output {
            Output::Text => {
                // Python format: {avatar_mxc}{SEP}{avatar_url}
                println!("{}{}{}", mxc_uri, sep, avatar_http);
            }
            Output::JsonSpec => (),
            _ => {
                print_json(
                    &serde_json::json!({
                        "avatar_url": mxc_uri.to_string(),
                        "avatar_http": avatar_http.as_str(),
                    }),
                    output,
                    false,
                );
            }
        }
        Ok(())
    } else {
        Err(Error::GetAvatarUrlFailed)
    }
}

/// Read the avatar from a file and send it to server to be used as avatar of the current user.
pub(crate) async fn set_avatar(
    client: &Client,
    path: &PathBuf,
    output: Output,
) -> Result<(), Error> {
    debug!("Upload avatar to server");
    let image = match fs::read(path) {
        Ok(image) => {
            debug!("Avatar file read successfully");
            image
        }
        Err(e) => return Err(Error::IO(e)),
    };
    if let Ok(mxc_uri) = client
        .account()
        .upload_avatar(
            &mime_guess::from_path(path).first_or(mime::IMAGE_PNG),
            image,
        )
        .await
    {
        debug!(
            "Avatar file uploaded successfully. MXC_URI is {:?}",
            mxc_uri
        );
        print_json(
            &serde_json::json!({"filename": path.to_str(), "avatar_mxc_uri": mxc_uri.to_string()}),
            output,
            false,
        );
        Ok(())
    } else {
        Err(Error::SetAvatarFailed)
    }
}

/// Send new MXC URI to server to be used as avatar of the current user.
pub(crate) async fn set_avatar_url(
    client: &Client,
    mxc_uri: &OwnedMxcUri,
    _output: Output,
) -> Result<(), Error> {
    debug!("Upload avatar MXC URI to server");
    if client.account().set_avatar_url(Some(mxc_uri)).await.is_ok() {
        debug!("Avatar file uploaded successfully.",);
        Ok(())
    } else {
        Err(Error::SetAvatarUrlFailed)
    }
}

/// Remove any MXC URI on server which are used as avatar of the current user.
/// In other words, remove the avatar from the matrix-commander-ng user.
pub(crate) async fn unset_avatar_url(client: &Client, _output: Output) -> Result<(), Error> {
    debug!("Remove avatar MXC URI on server");
    if client.account().set_avatar_url(None).await.is_ok() {
        debug!("Avatar removed successfully.",);
        Ok(())
    } else {
        Err(Error::UnsetAvatarUrlFailed)
    }
}

/// Get display name of the current user.
pub(crate) async fn get_display_name(client: &Client, output: Output, sep: &str) -> Result<(), Error> {
    debug!("Get display name from server");
    if let Ok(Some(name)) = client.account().get_display_name().await {
        debug!(
            "Display name obtained successfully. Display name is {:?}",
            name
        );
        match output {
            Output::Text => {
                // Match Python format: user_id    displayname
                let user_id = client.session_meta().unwrap().user_id.clone();
                println!("{}{}{}", user_id, sep, name);
            }
            Output::JsonSpec => (),
            _ => {
                let user_id = client.session_meta().unwrap().user_id.clone();
                print_json(&serde_json::json!({"displayname": name, "user": user_id.to_string()}), output, false);
            }
        }
        Ok(())
    } else {
        Err(Error::GetDisplaynameFailed)
    }
}

/// Set display name of the current user.
pub(crate) async fn set_display_name(
    client: &Client,
    name: &String,
    _output: Output,
) -> Result<(), Error> {
    debug!("Set display name of current user");
    if client.account().set_display_name(Some(name)).await.is_ok() {
        debug!("Display name set successfully.",);
        Ok(())
    } else {
        Err(Error::SetDisplaynameFailed)
    }
}

/// Get profile of the current user.
pub(crate) async fn get_profile(client: &Client, output: Output, sep: &str) -> Result<(), Error> {
    debug!("Get profile from server");
    if let Ok(profile) = client.account().fetch_user_profile().await {
        debug!("Profile successfully. Profile {:?}", profile);
        let displayname = profile
            .get("displayname")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let avatar_mxc = profile
            .get("avatar_url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Convert MXC URI to HTTP URL (like Python's mxc_to_http)
        let avatar_http = if !avatar_mxc.is_empty() {
            let mxc_owned = OwnedMxcUri::from(avatar_mxc.to_owned());
            if let Ok((server_name, media_id)) = mxc_owned.parts() {
                let hs = client.homeserver().to_string();
                let proto = hs.split("://").next().unwrap_or("https");
                format!(
                    "{}://{}/_matrix/media/r0/download/{}/{}",
                    proto, server_name, server_name, media_id
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        // Python: other_info is empty dict {} when no extra profile info
        // Python converts empty dict to "" for text and keeps {} for JSON
        // Collect other_info: any profile keys that aren't displayname/avatar_url
        let other_info_map: serde_json::Map<String, serde_json::Value> = profile
            .iter()
            .filter(|(k, _)| k.as_str() != "displayname" && k.as_str() != "avatar_url")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let other_info_text = if other_info_map.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&other_info_map).unwrap_or_default()
        };
        match output {
            Output::Text => {
                // Python format: {displayname}{SEP}{avatar_mxc}{SEP}{avatar_url}{SEP}{other_info}
                // Python prints None for null values
                println!(
                    "{}{}{}{}{}{}{}",
                    if displayname.is_empty() { "None" } else { displayname },
                    sep,
                    if avatar_mxc.is_empty() { "None" } else { avatar_mxc },
                    sep,
                    if avatar_http.is_empty() { "None".to_string() } else { avatar_http.clone() },
                    sep,
                    other_info_text,
                );
            }
            Output::JsonSpec => (),
            _ => {
                // Python uses None (null) for empty displayname and avatar_url
                let dn_val: serde_json::Value = if displayname.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(displayname.to_string())
                };
                let avatar_url_val: serde_json::Value = if avatar_mxc.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(avatar_mxc.to_string())
                };
                let avatar_http_val: serde_json::Value = if avatar_http.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(avatar_http)
                };
                let other_info_val: serde_json::Value = if other_info_map.is_empty() {
                    serde_json::Value::Object(serde_json::Map::new())
                } else {
                    serde_json::Value::Object(other_info_map)
                };
                print_json(
                    &serde_json::json!({
                        "displayname": dn_val,
                        "avatar_url": avatar_url_val,
                        "avatar_http": avatar_http_val,
                        "other_info": other_info_val,
                    }),
                    output,
                    false,
                );
            }
        }
        Ok(())
    } else {
        Err(Error::GetProfileFailed)
    }
}

fn obfuscate(text: &str, count: usize) -> String {
    let mut head: String = text.chars().take(count).collect();
    head.push_str("****");
    head
}

/// Redact a token for safe logging: shows first 4 and last 4 chars with "..." in the middle.
/// For tokens shorter than 12 chars, shows only first 4 chars + "...".
fn redact_token(token: &str) -> String {
    if token.len() > 12 {
        format!("{}...{}", &token[..4], &token[token.len() - 4..])
    } else if token.len() > 4 {
        format!("{}...", &token[..4])
    } else {
        "***".to_string()
    }
}

/// Get masterkey of the current user.
/// See: https://docs.rs/matrix-sdk/0.7.1/matrix_sdk/encryption/identities/struct.UserIdentity.html#method.master_key
pub(crate) async fn get_masterkey(
    client: &Client,
    userid: OwnedUserId,
    output: Output,
) -> Result<(), Error> {
    debug!("Get masterkey");

    match client.encryption().get_user_identity(&userid).await {
        Ok(Some(user)) => {
            // we fetch the first public key we
            // can find, there's currently only a single key allowed so this is
            // fine.
            match user.master_key().get_first_key().map(|k| k.to_base64()) {
                Some(masterkey) => {
                    debug!(
                        "get_masterkey obtained masterkey successfully. \
                        Masterkey {:?} (Obfuscated for privacy)",
                        obfuscate(&masterkey, 4)
                    );
                    print_json(&serde_json::json!({"masterkey": masterkey}), output, true);
                    Ok(())
                }
                None => {
                    error!("No masterkey available user {:?}", userid);
                    Err(Error::GetMasterkeyFailed)
                }
            }
        }
        Ok(None) => {
            error!("Error: user identity for user {:?} not found.", userid);
            Err(Error::GetMasterkeyFailed)
        }
        Err(e) => {
            error!(
                "Error: getting user identity for user {:?} failed. Error: {:?}",
                userid, e
            );
            Err(Error::GetMasterkeyFailed)
        }
    }
}

/// Get room info for a list of rooms.
/// Includes items such as room id, room display name, room alias, and room topic.
pub(crate) async fn get_room_info(
    client: &Client,
    rooms: &[String],
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    debug!("Getting room info");
    use matrix_sdk::deserialized_responses::SyncOrStrippedState;
    use matrix_sdk::ruma::events::room::create::RoomCreateEventContent;
    use matrix_sdk::ruma::events::room::guest_access::RoomGuestAccessEventContent;
    use matrix_sdk::ruma::events::room::history_visibility::RoomHistoryVisibilityEventContent;
    use matrix_sdk::ruma::events::SyncStateEvent;
    for (i, roomstr) in rooms.iter().enumerate() {
        debug!("Room number {} with room id {}", i, roomstr);
        let room_id = match RoomId::parse(roomstr.replace("\\!", "!")) {
            // remove possible escape
            Ok(ref inner) => inner.clone(),
            Err(ref e) => {
                error!("Invalid room id: {:?} {:?}", roomstr, e);
                continue;
            }
        };
        let room = client.get_room(&room_id).ok_or(Error::InvalidRoom)?;

        // Compute display_name matching Python: name or canonical_alias or group_name
        let display_name = match room.name() {
            Some(n) if !n.is_empty() => n,
            _ => match room.canonical_alias() {
                Some(alias) => alias.to_string(),
                None => room.display_name().await
                    .map(|dn| dn.to_string())
                    .unwrap_or_default(),
            },
        };

        match output {
            Output::Text => {
                let canonical_alias = room.canonical_alias()
                    .map_or(String::new(), |v| v.to_string());
                let topic = room.topic().unwrap_or_default();
                let encrypted = matches!(room.encryption_state(), EncryptionState::Encrypted);
                println!(
                    "{}{}{}{}{}{}{}{}{}",
                    room_id,
                    sep,
                    display_name,
                    sep,
                    if canonical_alias.is_empty() { "None".to_string() } else { canonical_alias },
                    sep,
                    if topic.is_empty() { "None".to_string() } else { topic },
                    sep,
                    if encrypted { "True" } else { "False" },
                );
            }
            Output::JsonSpec => (),
            _ => {
                let encrypted = matches!(room.encryption_state(), EncryptionState::Encrypted);
                let canonical_alias: serde_json::Value = match room.canonical_alias() {
                    Some(inner) => serde_json::Value::String(inner.to_string()),
                    None => serde_json::Value::Null,
                };
                let name: serde_json::Value = match room.name() {
                    Some(n) => serde_json::Value::String(n),
                    None => serde_json::Value::Null,
                };
                let topic: serde_json::Value = match room.topic() {
                    Some(t) if !t.is_empty() => serde_json::Value::String(t),
                    _ => serde_json::Value::Null,
                };
                let room_avatar_url: serde_json::Value = match room.avatar_url() {
                    Some(u) => serde_json::Value::String(u.to_string()),
                    None => serde_json::Value::Null,
                };
                let own_user_id = client.session_meta()
                    .map(|m| m.user_id.to_string())
                    .unwrap_or_default();
                let join_rule = room.join_rule()
                    .map(|jr| format!("{:?}", jr).to_lowercase())
                    .unwrap_or_else(|| "invite".to_string());

                // Fetch room_version from m.room.create state event
                let room_version = match room
                    .get_state_events_static::<RoomCreateEventContent>()
                    .await
                {
                    Ok(evs) => {
                        debug!("got {} create state events for room {}", evs.len(), room_id);
                        let mut ver = "1".to_string();
                        for ev in evs {
                            if let Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(original))) = ev.deserialize() {
                                ver = original.content.room_version.to_string();
                            }
                        }
                        ver
                    }
                    Err(e) => {
                        warn!("Failed to fetch create event for room {}: {}", room_id, e);
                        "1".to_string()
                    }
                };

                // Fetch guest_access from m.room.guest_access state event
                let guest_access = match room
                    .get_state_events_static::<RoomGuestAccessEventContent>()
                    .await
                {
                    Ok(evs) => {
                        debug!("got {} guest_access state events for room {}", evs.len(), room_id);
                        let mut ga = "forbidden".to_string();
                        for ev in evs {
                            if let Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(original))) = ev.deserialize() {
                                ga = original.content.guest_access.to_string();
                            }
                        }
                        ga
                    }
                    Err(e) => {
                        warn!("Failed to fetch guest_access for room {}: {}", room_id, e);
                        "forbidden".to_string()
                    }
                };

                // Fetch history_visibility from m.room.history_visibility state event
                let history_visibility = match room
                    .get_state_events_static::<RoomHistoryVisibilityEventContent>()
                    .await
                {
                    Ok(evs) => {
                        debug!("got {} history_visibility state events for room {}", evs.len(), room_id);
                        let mut hv = "shared".to_string();
                        for ev in evs {
                            if let Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(original))) = ev.deserialize() {
                                hv = original.content.history_visibility.to_string();
                            }
                        }
                        hv
                    }
                    Err(e) => {
                        warn!("Failed to fetch history_visibility for room {}: {}", room_id, e);
                        "shared".to_string()
                    }
                };

                // Fetch room members to build users and names maps
                // Like Python nio, request members from the server to ensure
                // we have the full member list (not just what's in the local store).
                let mut users_json = serde_json::Map::new();
                let mut names_json: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
                match room.members(RoomMemberships::JOIN).await {
                    Ok(members) => {
                        debug!("got {} joined members for room {}", members.len(), room_id);
                        for member in &members {
                            let uid = member.user_id().to_string();
                            let dname = member.display_name().map(|s| s.to_string());
                            let avatar = member.avatar_url().map(|u| u.to_string());
                            let power_level = match member.power_level() {
                                matrix_sdk::ruma::events::room::power_levels::UserPowerLevel::Int(n) => i64::from(n),
                                _ => 100, // Infinite or unknown future variants
                            };
                            let user_obj = serde_json::json!({
                                "user_id": uid,
                                "display_name": dname,
                                "avatar_url": avatar,
                                "power_level": power_level,
                                "invited": false,
                                "presence": "offline",
                                "last_active_ago": null,
                                "currently_active": null,
                                "status_msg": null,
                            });
                            users_json.insert(uid.clone(), user_obj);
                            if let Some(ref dn) = dname {
                                names_json.entry(dn.clone()).or_default().push(uid);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to fetch members for room {}: {}", room_id, e);
                    }
                }

                // Fetch power levels from room state
                let power_levels_json = match room
                    .get_state_events_static::<RoomPowerLevelsEventContent>()
                    .await
                {
                    Ok(evs) => {
                        let mut pl_json = serde_json::json!({});
                        for ev in evs {
                            if let Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(original))) = ev.deserialize() {
                                let pl = original.content;
                                let users_map: serde_json::Map<String, serde_json::Value> = pl.users.iter()
                                    .map(|(uid, level)| (uid.to_string(), serde_json::json!(i64::from(*level))))
                                    .collect();
                                let events_map: serde_json::Map<String, serde_json::Value> = pl.events.iter()
                                    .map(|(evt, level)| (evt.to_string(), serde_json::json!(i64::from(*level))))
                                    .collect();
                                pl_json = serde_json::json!({
                                    "defaults": {
                                        "ban": i64::from(pl.ban),
                                        "invite": i64::from(pl.invite),
                                        "kick": i64::from(pl.kick),
                                        "redact": i64::from(pl.redact),
                                        "state_default": i64::from(pl.state_default),
                                        "events_default": i64::from(pl.events_default),
                                        "users_default": i64::from(pl.users_default),
                                        "notifications": {
                                            "room": i64::from(pl.notifications.room),
                                        },
                                    },
                                    "users": users_map,
                                    "events": events_map,
                                });
                            }
                        }
                        pl_json
                    }
                    Err(e) => {
                        warn!("Failed to fetch power_levels for room {}: {}", room_id, e);
                        serde_json::json!({})
                    }
                };

                let parents_json = serde_json::json!({"set": "set()"});
                let children_json = serde_json::json!({"set": "set()"});
                // Python's RoomSummary defaults to None for all fields
                let summary_json = serde_json::json!({
                    "invited_member_count": null,
                    "joined_member_count": null,
                    "heroes": null,
                });
                let room_type: serde_json::Value = serde_json::Value::String(String::new());

                // Python: unread_notifications comes from sync response
                let unread_count = room.unread_notification_counts();
                let unread_notifications = unread_count.notification_count;
                let unread_highlights = unread_count.highlight_count;

                print_json(
                    &serde_json::json!({
                        "room_id": room_id.to_string(),
                        "own_user_id": own_user_id,
                        "federate": true,
                        "room_version": room_version,
                        "room_type": room_type,
                        "guest_access": guest_access,
                        "join_rule": join_rule,
                        "history_visibility": history_visibility,
                        "canonical_alias": canonical_alias,
                        "topic": topic,
                        "name": name,
                        "parents": parents_json,
                        "children": children_json,
                        "users": users_json,
                        "invited_users": {},
                        "names": names_json,
                        "encrypted": encrypted,
                        "power_levels": power_levels_json,
                        "typing_users": [],
                        "read_receipts": {},
                        "threaded_read_receipts": {},
                        "summary": summary_json,
                        "room_avatar_url": room_avatar_url,
                        "fully_read_marker": null,
                        "tags": {},
                        "unread_notifications": unread_notifications,
                        "unread_highlights": unread_highlights,
                        "members_synced": false,
                        "replacement_room": null,
                        "display_name": display_name,
                    }),
                    output,
                    false,
                );
            }
        };
    }
    Ok(())
}

/// Utility function to print JSON object as JSON or as plain text
/// Sometimes private sensitive data is being printed.
/// To avoid printing private keys or passwords, set obfuscated to true.
pub(crate) fn print_json(json_data: &serde_json::Value, output: Output, obfuscate: bool) {
    if obfuscate {
        debug!("Skipping printing this object due to privacy.")
    } else {
        debug!("{:?}", json_data);
    }
    match output {
        Output::Text => {
            if let serde_json::Value::Object(map) = json_data {
                let mut first = true;
                for (key, val) in map {
                    if first {
                        first = false;
                    } else {
                        print!("    ");
                    }
                    print!("{}:", key);
                    if val.is_object() {
                        print_json(val, output, obfuscate);
                    } else if val.is_boolean() {
                        print!("    {}", val);
                    } else if val.is_null() {
                        print!("    "); // print nothing
                    } else if val.is_string() {
                        print!("    {}", val.as_str().unwrap());
                    } else if val.is_number() {
                        print!("    {}", val);
                    } else if val.is_array() {
                        let items: Vec<String> = val.as_array().unwrap().iter().map(|v| {
                            if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() }
                        }).collect();
                        print!("    [{}]", items.join(", "));
                    }
                }
                println!();
            }
        }
        Output::JsonSpec => (),
        _ => {
            println!("{}", serde_json::to_string(json_data).unwrap_or_default());
        }
    }
}

/// Utility function to print Common room info
pub(crate) fn print_common_room(room: &room::Room, output: Output) {
    debug!("common room: {:?}", room);
    match output {
        Output::Text => {
            // Match Python format: just room_id per line
            println!("{}", room.room_id());
        }
        Output::JsonSpec => (),
        _ => {
            // println!(
            //                 "{{\"room_id\": {:?}, \"room_type\": {}, \"canonical_alias\": {:?}, \"alt_aliases\": {}, \"name\": {:?}, \"topic\": {:?}}}",
            //                 room.room_id(),
            //                 serde_json::to_string(&room.clone_info().room_type()).unwrap_or_else(|_| r#""""#.to_string()), // serialize, empty string as default
            //                 room.canonical_alias().map_or(r#""#.to_string(),|v|v.to_string()),
            //                 serde_json::to_string(&room.alt_aliases()).unwrap_or_else(|_| r#"[]"#.to_string()), // serialize, empty array as default
            //                 room.name().unwrap_or_default(),
            //                 room.topic().unwrap_or_default(),
            //             );
            #[derive(serde::Serialize)]
            struct MyRoom<'a> {
                room_id: &'a str,
                room_info: &'a matrix_sdk::RoomInfo,
                alt_aliases: Vec<OwnedRoomAliasId>,
            }
            let myroom = MyRoom {
                room_id: room.room_id().as_str(),
                room_info: &room.clone_info(),
                alt_aliases: room.alt_aliases(),
            };
            let jsonstr = serde_json::to_string(&myroom).unwrap();
            println!("{}", jsonstr);
        }
    }
}

/// Utility function to print Common room info of multiple rooms
/// Python outputs {"rooms": ["!room1:server", ...]} matching JoinedRoomsResponse.__dict__
pub(crate) fn print_common_rooms(rooms: Vec<room::Room>, output: Output) {
    debug!("common rooms: {:?}", rooms);
    // Deduplicate: the SDK may return the same room multiple times
    let mut seen = std::collections::HashSet::new();
    let rooms: Vec<_> = rooms
        .into_iter()
        .filter(|r| seen.insert(r.room_id().to_owned()))
        .collect();
    match output {
        Output::Text => {
            for r in &rooms {
                print_common_room(r, output)
            }
        }
        Output::JsonSpec => (),
        _ => {
            // Match Python format: {"rooms": ["!room1:server", "!room2:server", ...]}
            let room_ids: Vec<String> = rooms.iter().map(|r| r.room_id().to_string()).collect();
            print_json(
                &serde_json::json!({"rooms": room_ids}),
                output,
                false,
            );
        }
    }
}

/// Print list of rooms of a given type (invited, joined, left, all) of the current user.
pub(crate) fn print_rooms(
    client: &Client,
    rooms: Option<matrix_sdk::RoomState>, // None is the default and prints all 3 types of rooms
    output: Output,
) -> Result<(), Error> {
    debug!("Rooms (local)");
    match rooms {
        None => {
            // ALL rooms, default
            print_common_rooms(client.rooms(), output);
        }
        Some(matrix_sdk::RoomState::Invited) => {
            print_common_rooms(client.invited_rooms(), output);
        }
        Some(matrix_sdk::RoomState::Joined) => {
            print_common_rooms(client.joined_rooms(), output);
        }
        Some(matrix_sdk::RoomState::Left) => {
            print_common_rooms(client.left_rooms(), output);
        }
        Some(matrix_sdk::RoomState::Knocked) => {
            print_common_rooms(client.rooms_filtered(RoomStateFilter::KNOCKED), output);
        }
        Some(matrix_sdk::RoomState::Banned) => {
            print_common_rooms(client.rooms_filtered(RoomStateFilter::BANNED), output);
        }
    };
    Ok(())
}

/// Print list of all rooms (invited, joined, left) of the current user.
pub(crate) async fn rooms(client: &Client, output: Output) -> Result<(), Error> {
    debug!("Rooms (local)");
    print_rooms(client, None, output)
}

/// Print list of all invited rooms (not joined, not left) of the current user.
pub(crate) async fn invited_rooms(client: &Client, output: Output) -> Result<(), Error> {
    debug!("Invited_rooms (local)");
    print_rooms(client, Some(matrix_sdk::RoomState::Invited), output)
}

/// Print list of all joined rooms (not invited, not left) of the current user.
pub(crate) async fn joined_rooms(client: &Client, output: Output) -> Result<(), Error> {
    debug!("Joined_rooms (local)");
    print_rooms(client, Some(matrix_sdk::RoomState::Joined), output)
}

/// Print list of all left rooms (not invited, not joined) of the current user.
pub(crate) async fn left_rooms(client: &Client, output: Output) -> Result<(), Error> {
    debug!("Left_rooms (local)");
    print_rooms(client, Some(matrix_sdk::RoomState::Left), output)
}

/// Create rooms, either noemal room or DM room:
/// For normal room, create one room for each alias name in the list.
/// For DM room, create one DM room for each user name in the list.
/// Alias name can be empty, i.e. ''.
/// If and when available set the room name from the name list.
/// If and when available set the topic name from the topic list.
/// As output it lists/prints the newly generated room ids and and the corresponding room aliases.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn room_create(
    client: &Client,
    is_dm: bool,             // is DM room
    is_encrypted: bool,      // create an encrypted room or not
    users: &[String],        // users, only useful for DM rooms
    room_aliases: &[String], // list of simple alias names like 'SomeAlias', not full aliases
    names: &[String],        // list of room names, optional
    room_topics: &[String],  // list of room topics, optional
    output: Output,          // how to format output
    visibility: Visibility,  // visibility of the newly created room
    sep: &str,               // column separator
) -> Result<(), Error> {
    debug!("Creating room(s)");
    debug!(
        "Creating room(s): dm {:?}, u {:?}, a {:?}, n {:?}, t {:?}",
        is_dm, users, room_aliases, names, room_topics
    );
    // num...the number of rooms we are going to create
    let num = if is_dm {
        users.len()
    } else {
        room_aliases.len()
    };
    let mut users2 = Vec::new();
    let mut aliases2 = room_aliases.to_owned();
    let mut names2 = names.to_owned();
    let mut topics2 = room_topics.to_owned();
    if is_dm {
        aliases2.resize(num, "".to_string());
        users2.extend_from_slice(users);
    } else {
        users2.resize(num, "".to_string());
    }
    convert_to_short_canonical_alias_ids(&mut aliases2);
    names2.resize(num, "".to_string());
    topics2.resize(num, "".to_string());
    // all 4 vectors are now at least 'num' long; '' represents None
    let mut i = 0usize;
    let mut err_count = 0usize;
    debug!(
        "Creating room(s): dm {:?}, u {:?}, a {:?}, n {:?}, t {:?}, num {}",
        is_dm, users2, aliases2, names2, topics2, num
    );
    while i < num {
        debug!(
            "In position {} we have user {:?}, alias name {:?}, room name {:?}, room topic {:?}.",
            i, users2[i], aliases2[i], names2[i], topics2[i]
        );

        let mut request = CreateRoomRequest::new();
        let mut initstateevvec: Vec<Raw<AnyInitialStateEvent>> = vec![];
        if is_encrypted {
            // see: https://docs.rs/ruma/0.7.4/ruma/api/client/room/create_room/v3/struct.Request.html
            // pub struct Request<'a> {
            //     pub creation_content: Option<Raw<CreationContent>>,
            //     pub initial_state: &'a [Raw<AnyInitialStateEvent>],
            //     pub invite: &'a [OwnedUserId],
            //     pub invite_3pid: &'a [Invite3pid<'a>],
            //     pub is_direct: bool,
            //     pub name: Option<&'a str>,
            //     pub power_level_content_override: Option<Raw<RoomPowerLevelsEventContent>>,
            //     pub preset: Option<RoomPreset>,
            //     pub room_alias_name: Option<&'a str>,
            //     pub room_version: Option<&'a RoomVersionId>,
            //     pub topic: Option<&'a str>,
            //     pub visibility: Visibility,  }
            let content =
                RoomEncryptionEventContent::new(EventEncryptionAlgorithm::MegolmV1AesSha2);
            let initstateev = InitialStateEvent::new(EmptyStateKey, content);
            let rawinitstateev = Raw::new(&initstateev)?;
            // let anyinitstateev: AnyInitialStateEvent =
            //     matrix_sdk::ruma::events::AnyInitialStateEvent::RoomEncryption(initstateev);
            // todo: better alternative? let anyinitstateev2: AnyInitialStateEvent = AnyInitialStateEvent::from(initstateev);

            let rawanyinitstateev: Raw<AnyInitialStateEvent> = rawinitstateev.cast();
            initstateevvec.push(rawanyinitstateev);
            request.initial_state = initstateevvec;
        }

        request.name = Some(names2[i].clone());
        request.room_alias_name = Some(aliases2[i].clone()).filter(|s| !s.is_empty());
        request.topic = Some(topics2[i].clone());
        request.is_direct = is_dm;
        let usr: OwnedUserId;
        let mut invites = vec![];
        if is_dm {
            usr = match UserId::parse(<std::string::String as AsRef<str>>::as_ref(
                &users2[i].replace("\\@", "@"),
            )) {
                // remove possible escape
                Ok(u) => u,
                Err(ref e) => {
                    err_count += 1;
                    i += 1;
                    error!(
                        "Error: create_room failed, because user for DM is not valid, \
                        reported error {:?}.",
                        e
                    );
                    continue;
                }
            };
            invites.push(usr);
            request.invite = invites;
            // Visibility defaults to "Private" by matrix-sdk API, so "Private" for both normal rooms and DM rooms.
            request.visibility = visibility.clone();
            request.preset = match visibility {
                Visibility::Public => {
                    warn!(
                        "Creating a public room for a DM user is not allowed. Setting to private."
                    );
                    Some(RoomPreset::PrivateChat)
                }
                Visibility::Private => Some(RoomPreset::PrivateChat),
                _ => None,
            };
        } else {
            request.visibility = visibility.clone();
            request.preset = match visibility {
                Visibility::Public => {
                    info!(
                        "Creating a public {} room.",
                        if is_encrypted {
                            "encrypted"
                        } else {
                            "unencrypted"
                        }
                    );
                    Some(RoomPreset::PublicChat)
                }
                Visibility::Private => {
                    info!(
                        "Creating a private {} room.",
                        if is_encrypted {
                            "encrypted"
                        } else {
                            "unencrypted"
                        }
                    );
                    Some(RoomPreset::PrivateChat)
                }
                _ => None,
            };
        }
        match client.create_room(request).await {
            Ok(created_room) => {
                debug!("create_room succeeded, result is {:?}.", created_room);
                match output {
                    Output::Text => {
                        // Match Python format: room_id{SEP}alias{SEP}full_alias{SEP}name{SEP}topic{SEP}encrypted
                        let alias_str = if aliases2[i].is_empty() { "" } else { &aliases2[i] };
                        let full_alias = created_room.canonical_alias()
                            .map_or(String::new(), |v| v.to_string());
                        let name_str = if names2[i].is_empty() { "" } else { &names2[i] };
                        let topic_str = if topics2[i].is_empty() { "" } else { &topics2[i] };
                        let encrypted = matches!(created_room.encryption_state(), EncryptionState::Encrypted) || is_encrypted;
                        println!(
                            "{}{}{}{}{}{}{}{}{}{}{}",
                            created_room.room_id(),
                            sep,
                            alias_str,
                            sep,
                            full_alias,
                            sep,
                            name_str,
                            sep,
                            topic_str,
                            sep,
                            if encrypted { "True" } else { "False" },
                        );
                    }
                    _ => {
                        // Compute alias_full like Python: #alias:server_name
                        let alias_full = if !aliases2[i].is_empty() {
                            let server = client.user_id()
                                .map(|u| u.server_name().to_string())
                                .unwrap_or_default();
                            let short = &aliases2[i];
                            if short.starts_with('#') {
                                format!("{}:{}", short, server)
                            } else {
                                format!("#{}:{}", short, server)
                            }
                        } else {
                            String::new()
                        };
                        print_json(
                            &serde_json::json!({
                                "room_id": created_room.room_id().to_string(),
                                "alias": to_opt(&aliases2[i]),
                                "alias_full": to_opt(&alias_full),
                                "name": to_opt(&names2[i]),
                                "topic": to_opt(&topics2[i]),
                                "encrypted": matches!(created_room.encryption_state(), EncryptionState::Encrypted) || is_encrypted,
                            }),
                            output,
                            false,
                        );
                    }
                }
                // room_enable_encryption(): no longer needed, already done by setting request.initial_state
            }
            Err(ref e) => {
                err_count += 1;
                error!("Error: create_room failed, reported error {:?}.", e);
            }
        }
        i += 1;
    }
    if err_count != 0 {
        Err(Error::CreateRoomFailed)
    } else {
        Ok(())
    }
}

/// Leave room(s), leave all the rooms whose ids are given in the list.
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_leave(
    client: &Client,
    room_ids: &[String], // list of room ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!("Leaving room(s)");
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(match RoomId::parse(room_id.replace("\\!", "!")) {
            //remove possible escape
            Ok(id) => id,
            Err(ref e) => {
                error!(
                    "Error: invalid room id {:?}. Error reported is {:?}.",
                    room_id, e
                );
                err_count += 1;
                continue;
            }
        });
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        let jroomopt = client.get_room(id);
        match jroomopt {
            Some(jroom) => match jroom.leave().await {
                Ok(_) => {
                    info!("Left room {:?} successfully.", id);
                }
                Err(ref e) => {
                    error!("Error: leave() returned error {:?}.", e);
                    err_count += 1;
                }
            },
            None => {
                error!(
                    "Error: get_room() returned error. Only invited and joined rooms can be left."
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::LeaveRoomFailed)
    } else {
        Ok(())
    }
}

/// Forget room(s), forget all the rooms whose ids are given in the list.
/// Before you can forget a room you must leave it first.
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_forget(
    client: &Client,
    room_ids: &[String], // list of room ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!("Forgetting room(s)");
    let mut err_count = 0u32;
    debug!("All rooms of the default user: {:?}.", client.rooms());
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(match RoomId::parse(room_id.replace("\\!", "!")) {
            // remove possible escape
            Ok(id) => id,
            Err(ref e) => {
                error!(
                    "Error: invalid room id {:?}. Error reported is {:?}.",
                    room_id, e
                );
                err_count += 1;
                continue;
            }
        });
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        let jroomopt = client.get_room(id);
        match jroomopt {
            Some(jroom) => match jroom.forget().await {
                Ok(_) => {
                    info!("Forgot room {:?} successfully.", id);
                }
                Err(ref e) => {
                    error!("Error: forget() returned error {:?}.", e);
                    err_count += 1;
                }
            },
            None => {
                error!(
                    "Error: get_room() returned error. Have you been a member of \
                    this room? Have you left this room before? \
                    Leave the room before forgetting it."
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::ForgetRoomFailed)
    } else {
        Ok(())
    }
}

/// Invite user(s) into room(s).
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_invite(
    client: &Client,
    room_ids: &[String], // list of room ids
    user_ids: &[String], // list of user ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!(
        "Inviting user(s) to room(s): users={:?}, rooms={:?}",
        user_ids, room_ids
    );
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escapes
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    // convert Vec of strings into a slice of array of OwnedUserIds
    let mut userids: Vec<OwnedUserId> = Vec::new();
    for user_id in user_ids {
        userids.push(
            match UserId::parse(<std::string::String as AsRef<str>>::as_ref(
                &user_id.replace("\\@", "@"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid user id {:?}. Error reported is {:?}.",
                        user_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() || userids.is_empty() {
        if roomids.is_empty() {
            error!("No valid rooms. Cannot invite anyone. Giving up.")
        } else {
            error!("No valid users. Cannot invite anyone. Giving up.")
        }
        return Err(Error::InviteRoomFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        let jroomopt = client.get_room(id);
        match jroomopt {
            Some(jroom) => {
                for u in &userids {
                    match jroom.invite_user_by_id(u).await {
                        Ok(_) => {
                            info!("Invited user {:?} to room {:?} successfully.", u, id);
                        }
                        Err(ref e) => {
                            error!(
                                "Error: failed to invited user {:?} to room {:?}. \
                                invite_user_by_id() returned error {:?}.",
                                u, id, e
                            );
                            err_count += 1;
                        }
                    }
                }
            }
            None => {
                error!(
                    "Error: get_room() returned error. \
                    Are you a member of this room ({:?})? \
                    Join the room before inviting others to it.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::InviteRoomFailed)
    } else {
        Ok(())
    }
}

/// Join itself into room(s).
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_join(
    client: &Client,
    room_ids: &[String], // list of room ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!("Joining itself into room(s): rooms={:?}", room_ids);
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() {
        error!("No valid rooms. Cannot join any room. Giving up.");
        return Err(Error::JoinRoomFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        match client.join_room_by_id(id).await {
            Ok(_) => {
                info!("Joined room {:?} successfully.", id);
            }
            Err(ref e) => {
                error!(
                    "Error: failed to room {:?}. join_room_by_id() returned error {:?}.",
                    id, e
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::JoinRoomFailed)
    } else {
        Ok(())
    }
}

/// Ban user(s) from room(s).
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_ban(
    client: &Client,
    room_ids: &[String], // list of room ids
    user_ids: &[String], // list of user ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!(
        "Banning user(s) from room(s): users={:?}, rooms={:?}",
        user_ids, room_ids
    );
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    // convert Vec of strings into a slice of array of OwnedUserIds
    let mut userids: Vec<OwnedUserId> = Vec::new();
    for user_id in user_ids {
        userids.push(
            match UserId::parse(<std::string::String as AsRef<str>>::as_ref(
                &user_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid user id {:?}. Error reported is {:?}.",
                        user_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() || userids.is_empty() {
        if roomids.is_empty() {
            error!("No valid rooms. Cannot ban anyone. Giving up.")
        } else {
            error!("No valid users. Cannot ban anyone. Giving up.")
        }
        return Err(Error::BanRoomFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        let jroomopt = client.get_room(id);
        match jroomopt {
            Some(jroom) => {
                for u in &userids {
                    match jroom.ban_user(u, None).await {
                        Ok(_) => {
                            info!("Banned user {:?} from room {:?} successfully.", u, id);
                        }
                        Err(ref e) => {
                            error!(
                                "Error: failed to ban user {:?} from room {:?}. \
                                ban_user() returned error {:?}.",
                                u, id, e
                            );
                            err_count += 1;
                        }
                    }
                }
            }
            None => {
                error!(
                    "Error: get_room() returned error. Are you a member of this room ({:?})? \
                    Join the room before banning others from it.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::BanRoomFailed)
    } else {
        Ok(())
    }
}

/// Unbanning user(s) from room(s).
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_unban(
    client: &Client,
    room_ids: &[String], // list of room ids
    user_ids: &[String], // list of user ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!(
        "Unbanning user(s) from room(s): users={:?}, rooms={:?}",
        user_ids, room_ids
    );
    let mut err_count = 0u32;
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    let mut userids: Vec<OwnedUserId> = Vec::new();
    for user_id in user_ids {
        userids.push(
            match UserId::parse(<std::string::String as AsRef<str>>::as_ref(
                &user_id.replace("\\!", "!"),
            )) {
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid user id {:?}. Error reported is {:?}.",
                        user_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() || userids.is_empty() {
        if roomids.is_empty() {
            error!("No valid rooms. Cannot unban anyone. Giving up.")
        } else {
            error!("No valid users. Cannot unban anyone. Giving up.")
        }
        return Err(Error::UnbanRoomFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id);
        let jroomopt = client.get_room(id);
        match jroomopt {
            Some(jroom) => {
                for u in &userids {
                    match jroom.unban_user(u, None).await {
                        Ok(_) => {
                            info!("Unbanned user {:?} from room {:?} successfully.", u, id);
                        }
                        Err(ref e) => {
                            error!(
                                "Error: failed to unban user {:?} from room {:?}. \
                                unban_user() returned error {:?}.",
                                u, id, e
                            );
                            err_count += 1;
                        }
                    }
                }
            }
            None => {
                error!(
                    "Error: get_room() returned error. Are you a member of this room ({:?})? \
                    Join the room before unbanning others from it.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::UnbanRoomFailed)
    } else {
        Ok(())
    }
}

/// Kicking user(s) from room(s).
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn room_kick(
    client: &Client,
    room_ids: &[String], // list of room ids
    user_ids: &[String], // list of user ids
    _output: Output,     // how to format output, currently no output
) -> Result<(), Error> {
    debug!(
        "Kicking user(s) from room(s): users={:?}, rooms={:?}",
        user_ids, room_ids
    );
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    // convert Vec of strings into a slice of array of OwnedUserIds
    let mut userids: Vec<OwnedUserId> = Vec::new();
    for user_id in user_ids {
        userids.push(
            match UserId::parse(<std::string::String as AsRef<str>>::as_ref(
                &user_id.replace("\\@", "@"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid user id {:?}. Error reported is {:?}.",
                        user_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() || userids.is_empty() {
        if roomids.is_empty() {
            error!("No valid rooms. Cannot kick anyone. Giving up.")
        } else {
            error!("No valid users. Cannot kick anyone. Giving up.")
        }
        return Err(Error::KickRoomFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        let jroomopt = client.get_room(id);
        match jroomopt {
            Some(jroom) => {
                for u in &userids {
                    match jroom.kick_user(u, None).await {
                        Ok(_) => {
                            info!("Kicked user {:?} from room {:?} successfully.", u, id);
                        }
                        Err(ref e) => {
                            error!(
                                "Error: failed to kick user {:?} from room {:?}. \
                                kick_user() returned error {:?}.",
                                u, id, e
                            );
                            err_count += 1;
                        }
                    }
                }
            }
            None => {
                error!(
                    "Error: get_room() returned error. Are you a member of this room ({:?})? \
                    Join the room before kicking others from it.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::KickRoomFailed)
    } else {
        Ok(())
    }
}

/// Utility function to print visibility of a single room
fn print_room_visibility(room_id: &OwnedRoomId, room: &Room, output: Output, sep: &str) {
    match output {
        Output::Text => {
            // Match Python format: visibility    room_id
            println!(
                "{}{}{}",
                if room.is_public().unwrap_or(false) {
                    "public"
                } else {
                    "private"
                },
                sep,
                room_id,
            )
        }
        Output::JsonSpec => (),
        _ => {
            let visibility = if room.is_public().unwrap_or(false) {
                "public"
            } else {
                "private"
            };
            print_json(
                &serde_json::json!({
                    "room_id": room_id.to_string(),
                    "visibility": visibility,
                }),
                output,
                false,
            );
        }
    }
}

/// Listing visibility (public/private) for all room(s).
/// There will be one line printed per room.
pub(crate) async fn room_get_visibility(
    client: &Client,
    room_ids: &[String], // list of room ids
    output: Output,      // how to format output
    sep: &str,           // column separator
) -> Result<(), Error> {
    debug!("Get room visibility for room(s): rooms={:?}", room_ids);
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() {
        error!("No valid rooms. Cannot list anything. Giving up.");
        return Err(Error::RoomGetVisibilityFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        match client.get_room(id) {
            Some(r) => {
                print_room_visibility(id, &r, output, sep);
            }
            None => {
                error!(
                    "Error: failed to get room {:?}. get_room() returned error no room.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::RoomGetVisibilityFailed)
    } else {
        Ok(())
    }
}

/// Utility function to print part of the state of a single room
async fn print_room_state(room_id: &OwnedRoomId, room: &Room, output: Output, sep: &str) -> Result<(), Error> {
    // There are around 50 events for rooms
    // See https://docs.rs/ruma/0.7.4/ruma/?search=syncroom
    // We only do 4 as example to start with.
    let (member_evs, power_level_evs, name_evs, topic_evs) = tokio::try_join!(
        room.get_state_events_static::<RoomMemberEventContent>(),
        room.get_state_events_static::<RoomPowerLevelsEventContent>(),
        room.get_state_events_static::<RoomNameEventContent>(),
        room.get_state_events_static::<RoomTopicEventContent>(),
    )?;

    // Collect all events into a single JSON array to match Python's format:
    // Python outputs: {resp.events}{SEP}{room_id}
    // where resp.events is a list of raw state event dicts
    use matrix_sdk::deserialized_responses::RawSyncOrStrippedState;
    let mut all_events: Vec<serde_json::Value> = Vec::new();

    // Serialize each event type and collect into a single array
    for ev in &member_evs {
        if let Ok(val) = serde_json::to_value(ev) {
            all_events.push(val);
        }
    }
    for ev in &power_level_evs {
        if let Ok(val) = serde_json::to_value(ev) {
            all_events.push(val);
        }
    }
    for ev in &name_evs {
        if let Ok(val) = serde_json::to_value(ev) {
            all_events.push(val);
        }
    }
    for ev in &topic_evs {
        if let Ok(val) = serde_json::to_value(ev) {
            all_events.push(val);
        }
    }

    match output {
        Output::Text => {
            // Match Python format: events_json    room_id
            let events_json = serde_json::to_string(&all_events).unwrap_or_else(|_| "[]".to_string());
            println!("{}{}{}", events_json, sep, room_id);
        }
        // Output::JsonSpec => (), // These events should be spec compliant
        _ => {
            #[derive(serde::Serialize)]
            struct MyState<'a> {
                room_id: &'a str,
                room_member_event_content: Vec<RawSyncOrStrippedState<RoomMemberEventContent>>,
                room_power_levels_event_content:
                    Vec<RawSyncOrStrippedState<RoomPowerLevelsEventContent>>,
                room_name_event_content: Vec<RawSyncOrStrippedState<RoomNameEventContent>>,
                room_topic_event_content: Vec<RawSyncOrStrippedState<RoomTopicEventContent>>,
            }
            let mystate = MyState {
                room_id: room_id.as_str(),
                room_member_event_content: member_evs,
                room_power_levels_event_content: power_level_evs,
                room_name_event_content: name_evs,
                room_topic_event_content: topic_evs,
            };
            let jsonstr = serde_json::to_string(&mystate).unwrap();
            println!("{}", jsonstr);
        }
    }
    Ok(())
}

/// Listing partial state for all room(s).
/// There will be one line printed per room.
pub(crate) async fn room_get_state(
    client: &Client,
    room_ids: &[String], // list of room ids
    output: Output,      // how to format output
    sep: &str,           // column separator
) -> Result<(), Error> {
    debug!("Get room state for room(s): rooms={:?}", room_ids);
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() {
        error!("No valid rooms. Cannot list anything. Giving up.");
        return Err(Error::RoomGetStateFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        match client.get_room(id) {
            Some(r) => {
                if print_room_state(id, &r, output, sep).await.is_err() {
                    error!("Error: failed to get room state for room {:?}.", id);
                    err_count += 1;
                };
            }
            None => {
                error!(
                    "Error: failed to get room {:?}. get_room() returned error no room.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::RoomGetStateFailed)
    } else {
        Ok(())
    }
}

/// Utility function to print all members of a single room
fn print_room_members(room_id: &OwnedRoomId, members: &[RoomMember], output: Output, sep: &str) {
    match output {
        Output::Text => {
            // Match Python format:
            // room_id
            //     user_id    display_name    avatar_url
            //     user_id    display_name    avatar_url
            // (first line is room_id, each member line starts with SEP)
            let mut text = room_id.to_string();
            for m in members {
                text.push('\n');
                text.push_str(sep);
                text.push_str(m.user_id().as_str());
                text.push_str(sep);
                text.push_str(m.display_name().unwrap_or(""));
                text.push_str(sep);
                text.push_str(m.avatar_url().map_or("", |u| u.as_str()));
            }
            println!("{}", text);
        }
        Output::JsonSpec => (),
        _ => {
            // Match Python format: {room_id, members: [{user_id, display_name, avatar_url}]}
            // Python uses null for missing display_name/avatar_url
            let members_arr: Vec<serde_json::Value> = members.iter().map(|m| {
                serde_json::json!({
                    "user_id": m.user_id().as_str(),
                    "display_name": m.display_name(),
                    "avatar_url": m.avatar_url().map(|u| u.as_str().to_string()),
                })
            }).collect();
            let jobj = serde_json::json!({
                "room_id": room_id.as_str(),
                "members": members_arr,
            });
            print_json(&jobj, output, false);
        }
    }
}

/// Listing all joined member(s) for all room(s).
/// Does not list all members, e.g. does not list invited members, etc.
/// There will be one line printed per room.
pub(crate) async fn joined_members(
    client: &Client,
    room_ids: &[String], // list of room ids
    output: Output,      // how to format output
    sep: &str,           // column separator
) -> Result<(), Error> {
    debug!("Joined members for room(s): rooms={:?}", room_ids);
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                //remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() {
        error!("No valid rooms. Cannot kick anyone. Giving up.");
        return Err(Error::JoinedMembersFailed);
    }
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        match client.get_room(id) {
            Some(r) => match r.members(RoomMemberships::JOIN).await {
                Ok(ref m) => {
                    debug!("Members of room {:?} are {:?}.", id, m);
                    print_room_members(id, m, output, sep);
                }
                Err(ref e) => {
                    error!(
                        "Error: failed to get members of room {:?}. \
                        members() returned error {:?}.",
                        id, e
                    );
                    err_count += 1;
                }
            },
            None => {
                error!(
                    "Error: failed to get room {:?}. get_room() returned error no room.",
                    id
                );
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::JoinedMembersFailed)
    } else {
        Ok(())
    }
}

/// Get room name(s) based on room alias(es).
pub(crate) async fn room_resolve_alias(
    client: &Client,
    alias_ids: &[String], // list of room aliases
    output: Output,       // how to format output
    sep: &str,            // column separator
) -> Result<(), Error> {
    debug!("Resolving room alias(es)");
    let mut err_count = 0u32;
    debug!("Aliases given: {:?}.", alias_ids);
    // convert Vec of strings into a slice of array of OwnedRoomAliasIds
    let mut aliasids: Vec<OwnedRoomAliasId> = Vec::new();
    for alias_id in alias_ids {
        aliasids.push(
            match RoomAliasId::parse(alias_id.replace("\\#", "#")) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid alias id {:?}. Error reported is {:?}.",
                        alias_id, e
                    );
                    continue;
                }
            },
        );
    }
    for (i, id) in aliasids.iter().enumerate() {
        debug!("In position {} we have room alias id {:?}.", i, id,);
        match client.resolve_room_alias(id).await {
            Ok(res) => {
                info!("Resolved room alias {:?} successfully.", id);
                match output {
                    Output::Text => {
                        // Match Python format: alias    room_id    servers_list
                        // Python prints servers as Python list repr, e.g. ['server1', 'server2']
                        let servers_str: Vec<String> = res.servers.iter().map(|s| format!("'{}'", s)).collect();
                        println!("{}{}{}{}[{}]", id, sep, res.room_id, sep, servers_str.join(", "));
                    }
                    Output::JsonSpec => (),
                    _ => {
                        let servers: Vec<String> = res.servers.iter().map(|s| s.to_string()).collect();
                        print_json(
                            &serde_json::json!({
                                "room_alias": id.to_string(),
                                "room_id": res.room_id.to_string(),
                                "servers": servers,
                            }),
                            output,
                            false,
                        );
                    }
                }
            }
            Err(ref e) => {
                error!("Error: resolve_room_alias() returned error {:?}.", e);
                err_count += 1;
            }
        }
    }
    if err_count != 0 {
        Err(Error::ResolveRoomAliasFailed)
    } else {
        Ok(())
    }
}

/// Enable encryption for given room(s).
pub(crate) async fn room_enable_encryption(
    client: &Client,
    room_ids: &[String], // list of room ids
    _output: Output,     // how to format output
) -> Result<(), Error> {
    debug!("Enable encryption for room(s): rooms={:?}", room_ids);
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedRoomIds
    let mut roomids: Vec<OwnedRoomId> = Vec::new();
    for room_id in room_ids {
        roomids.push(
            match RoomId::parse(<std::string::String as AsRef<str>>::as_ref(
                &room_id.replace("\\!", "!"),
            )) {
                // remove possible escape
                Ok(id) => id,
                Err(ref e) => {
                    error!(
                        "Error: invalid room id {:?}. Error reported is {:?}.",
                        room_id, e
                    );
                    err_count += 1;
                    continue;
                }
            },
        );
    }
    if roomids.is_empty() {
        error!("No valid rooms. Cannot enable encryption anywhere. Giving up.");
        return Err(Error::EnableEncryptionFailed);
    }
    // without sync() client will not know that it is in joined rooms list and it will fail,
    // we must sync!
    // client.sync_once(SyncSettings::new()).await?; we should have sync-ed before.
    for (i, id) in roomids.iter().enumerate() {
        debug!("In position {} we have room id {:?}.", i, id,);
        match client.get_room(id) {
            Some(room) => match room.enable_encryption().await {
                Ok(_) => {
                    debug!("enable_encryption succeeded for room {:?}.", id);
                }
                Err(ref e) => {
                    err_count += 1;
                    error!(
                        "enable_encryption failed for room {:?} with reported error {:?}.",
                        id, e
                    );
                }
            },
            None => {
                err_count += 1;
                error!(
                    "get_room failed for room {:?}, \
                    Are you member of this room? \
                    If you are member of this room try syncing first.",
                    id
                );
            }
        }
    }
    if err_count != 0 {
        Err(Error::EnableEncryptionFailed)
    } else {
        Ok(())
    }
}

/// Pre-processing for Delete device(s).
/// This will adjust the lists for special shortcuts such as 'me' and '*'.
/// Get password and user if needed.
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn delete_devices_pre(client: &Client, ap: &mut Args) -> Result<(), Error> {
    debug!("Pre-processing for Deleting device(s)");
    get_password(ap);
    if let Some(user) = ap.user.first() {
        if let Some(password) = &ap.password {
            let mut hasstar = false;
            for i in &mut ap.delete_device {
                if i.to_lowercase() == "me" {
                    *i = ap.creds.as_ref().unwrap().device_id.to_string();
                }
                if i == "*" {
                    hasstar = true;
                }
            }
            if hasstar {
                ap.delete_device.retain(|x| x != "*");
                let response = client.devices().await?;
                for device in response.devices {
                    ap.delete_device.push(device.device_id.to_string());
                }
            }
            // hide password from debug log file
            debug!(
                "Preparing to delete these devices for user {:?} with password {:?}: {:?}",
                user, "******", ap.delete_device
            );
            delete_devices(client, &ap.delete_device, user, password, ap.output).await
        } else {
            Err(Error::MissingPassword)
        }
    } else {
        Err(Error::MissingUser)
    }
}

/// Delete device(s).
/// There is no output to stdout except debug and logging information.
/// If successful nothing will be output.
pub(crate) async fn delete_devices(
    client: &Client,
    device_ids: &[String], // list of device ids
    user: &str,
    password: &str,
    _output: Output, // how to format output, currently no output
) -> Result<(), Error> {
    debug!("Deleting device(s)");
    let mut err_count = 0u32;
    // convert Vec of strings into a slice of array of OwnedDeviceIds
    let mut deviceids: Vec<OwnedDeviceId> = Vec::new();
    for device_id in device_ids {
        let deviceid: OwnedDeviceId = device_id.as_str().into();
        deviceids.push(deviceid);
    }
    // hide password from debug log file
    debug!(
        "About to delete these devices of user {:?} with password {:?}: {:?}",
        user, "******", deviceids
    );
    if let Err(e) = client.delete_devices(&deviceids, None).await {
        if let Some(info) = e.as_uiaa_response() {
            let mut password = uiaa::Password::new(
                // full user id (@john:some.matrix.org), or just local part (john)
                uiaa::UserIdentifier::UserIdOrLocalpart(user.to_string()),
                password.to_string(),
            );
            password.session = info.session.clone();

            match client
                .delete_devices(&deviceids, Some(uiaa::AuthData::Password(password)))
                .await
            {
                Ok(_) => {
                    info!("Deleted devices {:?} successfully.", deviceids);
                }
                Err(ref e) => {
                    error!("Error: delete_devices() returned error {:?}.", e);
                    err_count += 1;
                }
            }
        }
    }
    if err_count != 0 {
        Err(Error::DeleteDeviceFailed)
    } else {
        Ok(())
    }
}

/// Send one or more text message
/// supporting various formats and types.
pub(crate) async fn message(
    client: &Client,
    msgs: &[String],
    roomnames: &[String],
    code: bool,
    markdown: bool,
    notice: bool,
    emote: bool,
    html: bool,
    print_event_id: bool,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    debug!(
        "In message(): roomnames are {:?}, msgs are {:?}",
        roomnames, msgs
    );
    if roomnames.is_empty() {
        return Err(Error::InvalidRoom);
    }
    let mut fmsgs: Vec<MessageType> = Vec::new(); // formatted msgs
    let mut fmt_msg: String;
    for msg in msgs.iter() {
        let (nmsg, md) = if code {
            fmt_msg = String::from("```");
            // fmt_msg.push_str("name-of-language");  // Todo
            fmt_msg.push('\n');
            fmt_msg.push_str(msg);
            if !fmt_msg.ends_with('\n') {
                fmt_msg.push('\n');
            }
            fmt_msg.push_str("```");
            (&fmt_msg, true)
        } else {
            (msg, markdown)
        };

        let fmsg = if notice {
            MessageType::Notice(if md {
                NoticeMessageEventContent::markdown(nmsg)
            } else if html {
                NoticeMessageEventContent::html(nmsg, nmsg)
            } else {
                NoticeMessageEventContent::plain(nmsg)
            })
        } else if emote {
            MessageType::Emote(if md {
                EmoteMessageEventContent::markdown(nmsg)
            } else if html {
                EmoteMessageEventContent::html(nmsg, nmsg)
            } else {
                EmoteMessageEventContent::plain(nmsg)
            })
        } else {
            MessageType::Text(if md {
                TextMessageEventContent::markdown(nmsg)
            } else if html {
                TextMessageEventContent::html(nmsg, nmsg)
            } else {
                TextMessageEventContent::plain(nmsg)
            })
        };
        fmsgs.push(fmsg);
    }
    if fmsgs.is_empty() {
        return Ok(()); // nothing to do
    }
    let mut err_count = 0u32;
    for roomname in roomnames.iter() {
        let proom = RoomId::parse(roomname.replace("\\!", "!")).unwrap(); // remove possible escape
        debug!("In message(): parsed room name is {:?}", proom);
        let room = client.get_room(&proom).ok_or(Error::InvalidRoom)?;
        for (fmsg, orig_msg) in fmsgs.iter().zip(msgs.iter()) {
            match room.send(RoomMessageEventContent::new(fmsg.clone())).await {
                Ok(response) => {
                    debug!("message send successful {:?}", response);
                    if print_event_id {
                        match output {
                            Output::Text => {
                                println!(
                                    "{}{}{}{}{}",
                                    response.event_id, sep,
                                    proom, sep,
                                    orig_msg,
                                );
                            }
                            Output::JsonSpec => (),
                            _ => {
                                print_json(
                                    &serde_json::json!({
                                        "event_id": response.event_id.to_string(),
                                        "room_id": proom.to_string(),
                                        "message": orig_msg.as_str(),
                                    }),
                                    output,
                                    false,
                                );
                            }
                        }
                    }
                }
                Err(ref e) => {
                    error!("message send returned error {:?}", e);
                    err_count += 1;
                }
            }
        }
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::SendFailed)
    }
}

/// Send one or more files,
/// allows various Mime formats.
// If a file is piped in from stdin, then use the 'stdin_filename' as label for the piped data.
// Implicitely this label also determines the MIME type of the piped data.
pub(crate) async fn file(
    client: &Client,
    filenames: &[PathBuf],
    roomnames: &[String],
    label: Option<&str>, // used as filename for attachment
    mime: Option<Mime>,
    stdin_filename: &PathBuf, // if a file is piped in on stdin
) -> Result<(), Error> {
    debug!(
        "In file(): roomnames are {:?}, files are {:?}",
        roomnames, filenames
    );
    if roomnames.is_empty() {
        return Err(Error::InvalidRoom);
    }
    if filenames.is_empty() {
        return Ok(()); // nothing to do
    }
    let mut err_count = 0u32;
    let mut pb: PathBuf;
    for roomname in roomnames.iter() {
        let proom = RoomId::parse(roomname.replace("\\!", "!")).unwrap(); // remove possible escape
        debug!("In file(): parsed room name is {:?}", proom);
        let room = client.get_room(&proom).ok_or(Error::InvalidRoom)?;
        for mut filename in filenames.iter() {
            let data = if filename.to_str().unwrap() == "-" {
                // read from stdin
                let mut buffer = Vec::new();
                if stdin().is_terminal() {
                    print!("Waiting for data to be piped into stdin. Enter data now: ");
                    std::io::stdout().flush()?;
                }
                // read the whole file
                io::stdin().read_to_end(&mut buffer)?;
                // change filename from "-" to "file" so that label shows up as "file"
                filename = stdin_filename;
                buffer
            } else {
                if filename.to_str().unwrap() == r"\-" {
                    pb = PathBuf::from(r"-").clone();
                    filename = &pb;
                }
                fs::read(filename).unwrap_or_else(|e| {
                    error!("file not found: {:?} {:?}", filename, e);
                    err_count += 1;
                    Vec::new()
                })
            };
            if data.is_empty() {
                error!("No data to send. Data is empty.");
                err_count += 1;
            } else {
                match room
                    .send_attachment(
                        label
                            .map(Cow::from)
                            .or_else(|| filename.file_name().as_ref().map(|o| o.to_string_lossy()))
                            .ok_or(Error::InvalidFile)?
                            .as_ref(),
                        mime.as_ref().unwrap_or(
                            &mime_guess::from_path(filename)
                                .first_or(mime::APPLICATION_OCTET_STREAM),
                        ),
                        data,
                        AttachmentConfig::new(),
                    )
                    .await
                {
                    Ok(response) => debug!("file send successful {:?}", response),
                    Err(ref e) => {
                        error!("file send returned error {:?}", e);
                        err_count += 1;
                    }
                }
            }
        }
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::SendFailed)
    }
}

/// Upload one or more files to the server.
/// Allows various Mime formats.
pub(crate) async fn media_upload(
    client: &Client,
    filenames: &[PathBuf],
    mime_strings: &[String],
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    debug!(
        "In media_upload(): filename are {:?}, mimes are {:?}",
        filenames, mime_strings,
    );
    let num = filenames.len();
    let mut i = 0usize;
    let mut mime_strings2 = mime_strings.to_owned();
    mime_strings2.resize(num, "".to_string());

    let mut err_count = 0u32;
    let mut filename;
    let mut mime_str;
    let mut mime;
    while i < num {
        filename = filenames[i].clone();
        mime_str = mime_strings2[i].clone();
        debug!(
            "In position {} we have filename {:?}, mime {:?}.",
            i, filename, mime_str
        );
        if mime_str.trim().is_empty() {
            mime = mime_guess::from_path(&filename).first_or(mime::APPLICATION_OCTET_STREAM);
        } else {
            mime = match mime_str.parse() {
                Ok(m) => m,
                Err(ref e) => {
                    error!(
                        "Provided Mime {:?} is not valid; the upload of file {:?} \
                        will be skipped; returned error {:?}",
                        mime_str, filename, e
                    );
                    err_count += 1;
                    i += 1;
                    continue;
                }
            }
        }

        let data = if filename.to_str().unwrap() == "-" {
            // read from stdin
            let mut buffer = Vec::new();
            if stdin().is_terminal() {
                eprint!("Waiting for data to be piped into stdin. Enter data now: ");
                std::io::stdout().flush()?;
            }
            // read the whole file
            io::stdin().read_to_end(&mut buffer)?;
            buffer
        } else {
            if filename.to_str().unwrap() == r"\-" {
                filename = PathBuf::from(r"-");
            }
            fs::read(&filename).unwrap_or_else(|e| {
                error!(
                    "File {:?} was not found; the upload of file {:?} \
                    will be skipped; returned error {:?}",
                    filename, filename, e
                );
                err_count += 1;
                Vec::new()
            })
        };
        if data.is_empty() {
            error!(
                "No data to send. Data is empty. The upload of file {:?} will be skipped.",
                filename
            );
            err_count += 1;
        } else {
            match client.media().upload(&mime, data, Some(client.request_config())).await {
                Ok(response) => {
                    debug!("upload successful {:?}", response);
                    // Match Python format: {content_uri}    {decryption_dict}
                    // Rust doesn't do encrypted uploads, so decryption_dict is None
                    match output {
                        Output::Text => {
                            println!("{}{}None", response.content_uri, sep);
                        }
                        Output::JsonSpec => (),
                        _ => {
                            print_json(
                                &serde_json::json!({
                                    "content_uri": response.content_uri.as_str(),
                                    "decryption_dict": null,
                                }),
                                output,
                                false,
                            );
                        }
                    }
                }
                Err(ref e) => {
                    error!(
                        "The upload of file {:?} failed. Upload returned error {:?}",
                        filename, e
                    );
                    err_count += 1;
                }
            }
        }
        i += 1;
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::MediaUploadFailed)
    }
}

/// Download one or more files from the server based on XMC URI.
/// Allows various Mime formats.
pub(crate) async fn media_download(
    client: &Client,
    mxc_uris: &[OwnedMxcUri],
    filenames: &[PathBuf],
    key_dicts: &[String],
    _output: Output, // how to format output; Python produces no stdout on download success
) -> Result<(), Error> {
    debug!(
        "In media_download(): mxc_uris are {:?}, filenames are {:?}, key_dicts count: {}",
        mxc_uris, filenames, key_dicts.len(),
    );
    let num = mxc_uris.len();
    let mut i = 0usize;
    let mut filenames2 = filenames.to_owned();
    filenames2.resize(num, PathBuf::new());
    let mut key_dicts2 = key_dicts.to_owned();
    key_dicts2.resize(num, String::new());

    let mut err_count = 0u32;
    let mut mxc_uri;
    let mut filename;
    while i < num {
        mxc_uri = mxc_uris[i].clone();
        filename = filenames2[i].clone();
        debug!(
            "In position {} we have mxc_uri {:?}, filename {:?}.",
            i, mxc_uri, filename
        );
        if filename.as_os_str().is_empty() {
            filename = PathBuf::from("mxc-".to_owned() + mxc_uri.media_id().unwrap_or(""));
        } else if filename.to_string_lossy().contains("__mxc_id__") {
            filename = PathBuf::from(filename.to_string_lossy().replacen(
                "__mxc_id__",
                mxc_uri.media_id().unwrap_or(""),
                10,
            ));
        }
        let key_dict_str = &key_dicts2[i];
        let encrypted = !key_dict_str.is_empty();
        let source = if encrypted {
            // Parse the key dictionary and build an EncryptedFile for decryption
            match parse_key_dict_to_encrypted_file(&mxc_uri, key_dict_str) {
                Ok(ef) => MediaSource::Encrypted(Box::new(ef)),
                Err(e) => {
                    error!(
                        "Failed to parse key dictionary for MXC URI {:?}: {}",
                        mxc_uri, e
                    );
                    err_count += 1;
                    i += 1;
                    continue;
                }
            }
        } else {
            if key_dicts.is_empty() {
                debug!(
                    "No key dictionary specified with --key-dict. \
                    Assuming download is not encrypted (plain-text). No decryption will be attempted."
                );
            }
            MediaSource::Plain(mxc_uri.clone())
        };
        let request = MediaRequestParameters {
            source,
            format: MediaFormat::File,
        };
        match client.media().get_media_content(&request, false).await {
            Ok(response) => {
                debug!("dowload successful: {:?} bytes received", response.len());
                if filename.to_str().unwrap() == "-" {
                    match std::io::stdout().write_all(&response) {
                        Ok(_) => {
                            debug!("Downloaded media was successfully written to stdout.");
                            // Python does not produce stdout output on download success
                        }
                        Err(ref e) => {
                            error!(
                                "The downloaded media data could not be written to stdout. \
                                write() returned error {:?}",
                                e
                            );
                            err_count += 1;
                            continue;
                        }
                    }
                } else {
                    if filename.to_str().unwrap() == r"\-" {
                        filename = PathBuf::from(r"-");
                    }
                    match File::create(&filename).map(|mut o| o.write_all(&response)) {
                        Ok(Ok(())) => {
                            debug!(
                                "Downloaded media was successfully written to file {:?}.",
                                filename
                            );
                            if response.is_empty() {
                                warn!("The download of MXC URI had 0 bytes of data. It is empty.");
                            };
                            // Python does not produce stdout output on download success
                        }
                        Ok(Err(ref e)) => {
                            error!(
                                "Writing downloaded media to file {:?} failed. \
                                Error returned is {:?}",
                                filename, e
                            );
                            err_count += 1;
                        }
                        Err(ref e) => {
                            error!(
                                "Could not create file {:?} for storing downloaded media. \
                                Returned error {:?}.",
                                filename, e
                            );
                            err_count += 1;
                        }
                    }
                };
            }
            Err(ref e) => {
                error!(
                    "The download of MXC URI {:?} failed. Download returned error {:?}",
                    mxc_uri, e
                );
                err_count += 1;
            }
        }
        i += 1;
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::MediaDownloadFailed)
    }
}

// Todo: remove media content thumbnails

/// Delete one or more files from the server based on XMC URI.
/// Does not delete Thumbnails.
pub(crate) async fn media_delete(
    client: &Client,
    mxc_uris: &[OwnedMxcUri],
    _output: Output, // how to format output
) -> Result<(), Error> {
    debug!("In media_delete(): mxc_uris are {:?}", mxc_uris,);
    let mut err_count = 0u32;
    for mxc in mxc_uris {
        match mxc.validate() {
            Ok(()) => {
                debug!("mxc {:?} is valid.", mxc);
                match client.media().remove_media_content_for_uri(mxc).await {
                    Ok(()) => {
                        debug!("Successfully deleted MXC URI {:?}.", mxc);
                    }
                    Err(ref e) => {
                        error!(
                            "Deleting the MXC URI {:?} failed. Error returned is {:?}.",
                            mxc, e
                        );
                        err_count += 1;
                    }
                }
            }
            Err(ref e) => {
                error!("Invalid MXC URI {:?}. Error returned is {:?}.", mxc, e);
                err_count += 1;
            }
        }
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::MediaDeleteFailed)
    }
}

/// Convert one or more XMC URIs to HTTP URLs. This is for legacy reasons
/// and compatibility with Python version of matrix-commander.
/// This works without a server and without being logged in.
/// Converts a string like "mxc://matrix.server.org/SomeStrangeUriKey"
/// to a string like "https://matrix.server.org/_matrix/media/r0/download/matrix.server.org/SomeStrangeUriKey".
pub(crate) async fn media_mxc_to_http(
    mxc_uris: &[OwnedMxcUri],
    default_homeserver: &Url,
    output: Output, // how to format output
    sep: &str,
) -> Result<(), Error> {
    debug!("In media_mxc_to_http(): mxc_uris are {:?}", mxc_uris,);
    let mut err_count = 0u32;
    let mut http;
    for mxc in mxc_uris {
        match mxc.validate() {
            Ok(()) => {
                let p = default_homeserver.as_str()
                    [0..default_homeserver.as_str().find('/').unwrap() - 1]
                    .to_string(); // http or https
                let (server_name, media_id) = mxc.parts().unwrap();
                debug!(
                    "MXC URI {:?} is valid. Protocol is {:?}, Server is {:?}, media id is {:?}.",
                    mxc, p, server_name, media_id
                );
                http = p
                    + "://"
                    + server_name.as_str()
                    + "/_matrix/media/r0/download/"
                    + server_name.as_str()
                    + "/"
                    + media_id;
                debug!("http of mxc {:?} is {:?}", mxc, http);
                // Match Python format: {mxc}    {http}
                match output {
                    Output::Text => {
                        println!("{}{}{}", mxc, sep, http);
                    }
                    Output::JsonSpec => (),
                    _ => {
                        print_json(
                            &serde_json::json!({"mxc": mxc.as_str(), "http": http.as_str()}),
                            output,
                            false,
                        );
                    }
                }
            }
            Err(ref e) => {
                error!("Invalid MXC URI {:?}. Error returned is {:?}.", mxc, e);
                err_count += 1;
            }
        }
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::MediaMxcToHttpFailed)
    }
}

/// Send one or more raw Matrix events.
/// Each event is a JSON string containing "type" and "content" fields.
pub(crate) async fn event(
    client: &Client,
    events: &[String],
    roomnames: &[String],
) -> Result<(), Error> {
    debug!(
        "In event(): roomnames are {:?}, events count {}",
        roomnames,
        events.len()
    );
    if roomnames.is_empty() {
        return Err(Error::InvalidRoom);
    }
    let mut err_count = 0u32;
    for roomname in roomnames.iter() {
        let proom = RoomId::parse(roomname.replace("\\!", "!")).unwrap();
        let room = client.get_room(&proom).ok_or(Error::InvalidRoom)?;
        for ev_json_str in events.iter() {
            let ev_json: serde_json::Value = match serde_json::from_str(ev_json_str) {
                Ok(v) => v,
                Err(e) => {
                    error!("Error: invalid JSON for event: {:?}. Error: {:?}", ev_json_str, e);
                    err_count += 1;
                    continue;
                }
            };
            let event_type = match ev_json.get("type").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => {
                    error!("Error: event JSON missing 'type' field: {:?}", ev_json_str);
                    err_count += 1;
                    continue;
                }
            };
            let content = match ev_json.get("content") {
                Some(c) => c.clone(),
                None => {
                    error!("Error: event JSON missing 'content' field: {:?}", ev_json_str);
                    err_count += 1;
                    continue;
                }
            };
            debug!("Sending event type {:?} with content {:?}", event_type, content);
            // Use the raw send API
            match room
                .send_raw(&event_type, content)
                .await
            {
                Ok(response) => {
                    info!("Event sent successfully. Event ID: {:?}", response.event_id);
                }
                Err(ref e) => {
                    error!("Error: event send failed: {:?}", e);
                    err_count += 1;
                }
            }
        }
    }
    if err_count == 0 {
        Ok(())
    } else {
        Err(Error::EventSendFailed)
    }
}

/// Import E2E room keys from a file using a passphrase.
pub(crate) async fn import_keys(
    client: &Client,
    file_path: &str,
    passphrase: &str,
    _output: Output,
) -> Result<(), Error> {
    info!("Importing room keys from {:?}", file_path);
    let path = PathBuf::from(file_path);
    match client
        .encryption()
        .import_room_keys(path, passphrase)
        .await
    {
        Ok(result) => {
            // Match Python: "Successfully imported keys from file {file}."
            info!("Successfully imported keys from file {}.", file_path);
            debug!(
                "Import details: {} of {} total keys imported.",
                result.imported_count, result.total_count
            );
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to import room keys: {:?}", e);
            Err(Error::ImportKeysFailed)
        }
    }
}

/// Export E2E room keys to a file using a passphrase.
pub(crate) async fn export_keys(
    client: &Client,
    file_path: &str,
    passphrase: &str,
    _output: Output,
) -> Result<(), Error> {
    info!("Exporting room keys to {:?}", file_path);
    let path = PathBuf::from(file_path);
    match client.encryption().export_room_keys(path, passphrase, |_| true).await {
        Ok(()) => {
            // Match Python: "Successfully exported keys to file {file}."
            info!("Successfully exported keys to file {}.", file_path);
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to export room keys: {:?}", e);
            Err(Error::ExportKeysFailed)
        }
    }
}

/// Get an OpenID token for the current user.
pub(crate) async fn get_openid_token(
    client: &Client,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    info!("Getting OpenID token");
    let user_id = client.user_id().ok_or(Error::NotLoggedIn)?;
    // Use the ruma API to request an OpenID token
    let request = matrix_sdk::ruma::api::client::account::request_openid_token::v3::Request::new(
        user_id.to_owned(),
    );
    match client.send(request).await {
        Ok(response) => {
            match output {
                Output::Text => {
                    // Python format: {user_id}{SEP}{access_token}{SEP}{expires_in}{SEP}{matrix_server_name}{SEP}{token_type}
                    println!(
                        "{}{}{}{}{}{}{}{}{}",
                        user_id, sep,
                        response.access_token, sep,
                        response.expires_in.as_secs(), sep,
                        response.matrix_server_name, sep,
                        response.token_type,
                    );
                }
                Output::JsonSpec => (),
                _ => {
                    print_json(
                        &serde_json::json!({
                            "user_id": user_id.as_str(),
                            "access_token": response.access_token.as_str(),
                            "expires_in": response.expires_in.as_secs(),
                            "matrix_server_name": response.matrix_server_name.as_str(),
                            "token_type": response.token_type.to_string(),
                        }),
                        output,
                        false,
                    );
                }
            }
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to get OpenID token: {:?}", e);
            Err(Error::GetOpenIdTokenFailed)
        }
    }
}

/// Set device display name
pub(crate) async fn set_device_name(
    client: &Client,
    device_name: &str,
    _output: Output,
) -> Result<(), Error> {
    info!("Setting device display name to {:?}", device_name);
    let device_id = client
        .device_id()
        .ok_or(Error::NotLoggedIn)?
        .to_owned();
    let mut request =
        matrix_sdk::ruma::api::client::device::update_device::v3::Request::new(device_id);
    request.display_name = Some(device_name.to_owned());
    match client.send(request).await {
        Ok(_) => {
            // Python prints nothing on success, only debug log
            debug!("update_device successful");
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to set device name: {:?}", e);
            Err(Error::SetDeviceNameFailed)
        }
    }
}

/// Set user presence status
pub(crate) async fn set_presence(
    client: &Client,
    presence: &Presence,
    _output: Output,
) -> Result<(), Error> {
    info!("Setting presence to {:?}", presence);
    let user_id = client.user_id().ok_or(Error::NotLoggedIn)?.to_owned();
    let sdk_presence = match presence {
        Presence::Online => matrix_sdk::ruma::presence::PresenceState::Online,
        Presence::Offline => matrix_sdk::ruma::presence::PresenceState::Offline,
        Presence::Unavailable => matrix_sdk::ruma::presence::PresenceState::Unavailable,
    };
    let request = matrix_sdk::ruma::api::client::presence::set_presence::v3::Request::new(
        user_id,
        sdk_presence,
    );
    match client.send(request).await {
        Ok(_) => {
            // Python prints nothing on success, only debug log
            debug!("set_presence successful");
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to set presence: {:?}", e);
            Err(Error::SetPresenceFailed)
        }
    }
}

/// Get user presence status
pub(crate) async fn get_presence(
    client: &Client,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    info!("Getting presence");
    let user_id = client.user_id().ok_or(Error::NotLoggedIn)?.to_owned();
    let request =
        matrix_sdk::ruma::api::client::presence::get_presence::v3::Request::new(user_id.clone());
    match client.send(request).await {
        Ok(response) => {
            let presence_str = response.presence.to_string();
            let status_msg = response.status_msg.as_deref().unwrap_or("");
            let last_active_ago: u64 = response
                .last_active_ago
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let currently_active = response.currently_active.unwrap_or(false);
            match output {
                Output::Text => {
                    // Python format: {user_id}{SEP}{presence}{SEP}{last_active_ago}{SEP}{currently_active}{SEP}{status_msg}
                    // Python uses True/False for booleans
                    println!(
                        "{}{}{}{}{}{}{}{}{}",
                        user_id, sep,
                        presence_str, sep,
                        last_active_ago, sep,
                        if currently_active { "True" } else { "False" }, sep,
                        status_msg
                    );
                }
                Output::JsonSpec => (),
                _ => {
                    print_json(
                        &serde_json::json!({
                            "user_id": user_id.as_str(),
                            "presence": presence_str.as_str(),
                            "last_active_ago": last_active_ago,
                            "currently_active": currently_active,
                            "status_msg": status_msg,
                        }),
                        output,
                        false,
                    );
                }
            }
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to get presence: {:?}", e);
            Err(Error::GetPresenceFailed)
        }
    }
}

/// Set a room alias
pub(crate) async fn room_set_alias(
    client: &Client,
    room_ids: &[String],
    aliases: &[String],
    _output: Output,
) -> Result<(), Error> {
    info!("Setting room alias");
    if room_ids.is_empty() || aliases.is_empty() {
        return Err(Error::MissingCliParameter);
    }
    let room_id = RoomId::parse(&room_ids[0]).map_err(|_| Error::InvalidRoom)?;
    for alias_str in aliases {
        let alias = RoomAliasId::parse(alias_str).map_err(|_| Error::RoomSetAliasFailed)?;
        let request =
            matrix_sdk::ruma::api::client::alias::create_alias::v3::Request::new(
                alias.to_owned(),
                room_id.to_owned(),
            );
        match client.send(request).await {
            Ok(_) => {
                // Match Python: only log on success, no stdout output
                info!(
                    "Successfully added alias '{}' to room '{}'.",
                    alias_str, room_ids[0]
                );
            }
            Err(e) => {
                error!("Error: failed to set room alias {:?}: {:?}", alias_str, e);
                return Err(Error::RoomSetAliasFailed);
            }
        }
    }
    Ok(())
}

/// Delete a room alias
pub(crate) async fn room_delete_alias(
    client: &Client,
    aliases: &[String],
    _output: Output,
) -> Result<(), Error> {
    info!("Deleting room alias");
    for alias_str in aliases {
        let alias = RoomAliasId::parse(alias_str).map_err(|_| Error::RoomDeleteAliasFailed)?;
        let request =
            matrix_sdk::ruma::api::client::alias::delete_alias::v3::Request::new(
                alias.to_owned(),
            );
        match client.send(request).await {
            Ok(_) => {
                // Match Python: only log on success, no stdout output
                info!("Successfully deleted room alias '{}'.", alias_str);
            }
            Err(e) => {
                error!(
                    "Error: failed to delete room alias {:?}: {:?}",
                    alias_str, e
                );
                return Err(Error::RoomDeleteAliasFailed);
            }
        }
    }
    Ok(())
}

/// Get server discovery information.
/// Matches Python's --discovery-info which calls /.well-known/matrix/client
/// and prints homeserver_url{SEP}identity_server_url.
pub(crate) async fn discovery_info(
    client: &Client,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    info!("Getting discovery info");
    // Try the well-known endpoint first (matches Python's client.discovery_info())
    let request =
        matrix_sdk::ruma::api::client::discovery::discover_homeserver::Request::new();
    match client.send(request).await {
        Ok(response) => {
            let homeserver_url = response.homeserver.base_url.trim_end_matches('/').to_string();
            let identity_server_url_opt = response
                .identity_server
                .map(|is| is.base_url.trim_end_matches('/').to_string())
                .filter(|s| !s.is_empty());
            match output {
                Output::Text => {
                    // Match Python format: {homeserver_url}    {identity_server_url}
                    // Python prints None for absent identity server
                    let is_text = identity_server_url_opt.as_deref().unwrap_or("None");
                    println!("{}{}{}", homeserver_url, sep, is_text);
                }
                Output::JsonSpec => (),
                _ => {
                    // Python uses null for absent identity server, not string "None"
                    let is_val: serde_json::Value = match &identity_server_url_opt {
                        Some(url) => serde_json::Value::String(url.clone()),
                        None => serde_json::Value::Null,
                    };
                    print_json(
                        &serde_json::json!({
                            "homeserver_url": homeserver_url.as_str(),
                            "identity_server_url": is_val,
                        }),
                        output,
                        false,
                    );
                }
            }
            Ok(())
        }
        Err(e) => {
            // Fallback: use client.homeserver() if well-known is not available
            debug!("Well-known endpoint failed ({:?}), falling back to client.homeserver()", e);
            let homeserver_url = client.homeserver().to_string();
            let homeserver_url = homeserver_url.trim_end_matches('/');
            match output {
                Output::Text => {
                    println!("{}{}None", homeserver_url, sep);
                }
                Output::JsonSpec => (),
                _ => {
                    print_json(
                        &serde_json::json!({
                            "homeserver_url": homeserver_url,
                            "identity_server_url": null,
                        }),
                        output,
                        false,
                    );
                }
            }
            Ok(())
        }
    }
}

/// Get login info (supported login types)
pub(crate) async fn login_info(
    client: &Client,
    output: Output,
) -> Result<(), Error> {
    info!("Getting login info");
    let request =
        matrix_sdk::ruma::api::client::session::get_login_types::v3::Request::new();
    match client.send(request).await {
        Ok(response) => {
            match output {
                Output::Text => {
                    // Match Python: each flow type string on its own line
                    // e.g. "m.login.password"
                    let mut text_parts: Vec<String> = Vec::new();
                    for flow in &response.flows {
                        text_parts.push(flow.login_type().to_string());
                    }
                    let text = text_parts.join("\n");
                    println!("{}", text);
                }
                Output::JsonSpec => (),
                _ => {
                    let flow_strs: Vec<serde_json::Value> =
                        response.flows.iter().map(|f| serde_json::Value::String(f.login_type().to_string())).collect();
                    print_json(
                        &serde_json::json!({
                            "flows": flow_strs,
                        }),
                        output,
                        false,
                    );
                }
            }
            Ok(())
        }
        Err(e) => {
            error!("Error: failed to get login info: {:?}", e);
            Err(Error::LoginInfoFailed)
        }
    }
}

/// Get content repository configuration (max upload size etc.)
pub(crate) async fn content_repository_config(
    client: &Client,
    output: Output,
) -> Result<(), Error> {
    info!("Getting content repository config");
    let request =
        matrix_sdk::ruma::api::client::authenticated_media::get_media_config::v1::Request::new();
    match client.send(request).await {
        Ok(response) => {
            let max_size = response.upload_size;
            match output {
                Output::Text => {
                    println!("{}", max_size);
                }
                Output::JsonSpec => (),
                _ => {
                    print_json(
                        &serde_json::json!({
                            "upload_size": u64::from(max_size),
                        }),
                        output,
                        false,
                    );
                }
            }
            Ok(())
        }
        Err(e) => {
            error!(
                "Error: failed to get content repository config: {:?}",
                e
            );
            Err(Error::ContentRepositoryConfigFailed)
        }
    }
}

/// Delete media uploaded before a given timestamp
pub(crate) async fn delete_mxc_before(
    _client: &Client,
    args: &[String],
    credentials: &Credentials,
    output: Output,
) -> Result<(), Error> {
    info!("Deleting MXC before timestamp");
    // Python format: --delete-mxc-before "TIMESTAMP" [SIZE]
    // TIMESTAMP: "DD.MM.YYYY HH:MM:SS" or epoch millis
    // SIZE: optional minimum size in bytes (default 0)
    if args.is_empty() || args.len() > 2 {
        error!(
            "Incorrect number of arguments for --delete-mxc-before. \
            There must be 1 or 2 arguments, but found {} arguments.",
            args.len()
        );
        return Err(Error::MissingCliParameter);
    }

    let ts_str = &args[0];
    // Try parsing as epoch millis first, then as date string
    let ts_ms: u64 = if let Ok(ms) = ts_str.parse::<u64>() {
        ms
    } else {
        // Try Python-compatible format: "DD.MM.YYYY HH:MM:SS"
        match chrono::NaiveDateTime::parse_from_str(ts_str, "%d.%m.%Y %H:%M:%S") {
            Ok(dt) => (dt.and_utc().timestamp() * 1000) as u64,
            Err(_) => {
                // Try ISO format: "YYYY-MM-DDTHH:MM:SS"
                match chrono::NaiveDateTime::parse_from_str(ts_str, "%Y-%m-%dT%H:%M:%S") {
                    Ok(dt) => (dt.and_utc().timestamp() * 1000) as u64,
                    Err(_) => {
                        error!(
                            "Error: invalid timestamp {:?}. Use epoch millis, \
                            'DD.MM.YYYY HH:MM:SS', or 'YYYY-MM-DDTHH:MM:SS'.",
                            ts_str
                        );
                        return Err(Error::DeleteMxcBeforeFailed);
                    }
                }
            }
        }
    };

    let size: u64 = if args.len() == 2 {
        args[1].parse().unwrap_or(0)
    } else {
        0
    };

    debug!(
        "Deleting media older than {} ms (size > {})",
        ts_ms, size
    );

    // POST /_synapse/admin/v1/media/<server_name>/delete?before_ts=<ts>&size_gt=<size>
    let mut homeserver = credentials.homeserver.to_string();
    if homeserver.ends_with('/') {
        homeserver.pop();
    }
    let server_name = credentials
        .homeserver
        .host_str()
        .unwrap_or("localhost");

    let url = format!(
        "{}/_synapse/admin/v1/media/{}/delete?before_ts={}&size_gt={}",
        homeserver, server_name, ts_ms, size
    );

    let http_client = reqwest::Client::new();
    match http_client
        .post(&url)
        .header("Authorization", format!("Bearer {}", credentials.access_token))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.is_success() {
                match output {
                    Output::Text => println!("{}", body),
                    Output::JsonSpec => (),
                    _ => println!("{}", body),
                }
                Ok(())
            } else {
                error!(
                    "Error: Synapse admin API returned status {}. Response: {}",
                    status, body
                );
                match output {
                    Output::Text => println!("Error: {} {}", status, body),
                    Output::JsonSpec => (),
                    _ => println!("{}", body),
                }
                Err(Error::DeleteMxcBeforeFailed)
            }
        }
        Err(e) => {
            error!("Error: failed to call Synapse admin API: {:?}", e);
            Err(Error::DeleteMxcBeforeFailed)
        }
    }
}

/// Get client info (user_id, device_id, homeserver, room_id, access_token, etc.)
/// Prints JSON-formatted client information, matching Python's action_get_client_info.
pub(crate) async fn get_client_info(client: &Client, ap: &Args) -> Result<(), Error> {
    debug!("Getting client info.");
    let creds = ap.creds.as_ref().ok_or(Error::NoCredentialsFound)?;

    let user_id = creds.user_id.to_string();
    let device_id = creds.device_id.to_string();
    let mut hs = creds.homeserver.to_string();
    if hs.ends_with('/') {
        hs.pop();
    }
    let room_id = creds.room_id.clone();
    // Obfuscate access token for display
    let access_token_display = if creds.access_token.len() > 8 {
        format!("{}...{}", &creds.access_token[..4], &creds.access_token[creds.access_token.len()-4..])
    } else {
        "***".to_string()
    };

    // Collect joined rooms
    let joined: Vec<String> = client.joined_rooms().iter().map(|r| r.room_id().to_string()).collect();

    let info_obj = serde_json::json!({
        "user_id": user_id,
        "device_id": device_id,
        "homeserver": hs,
        "room_id": room_id,
        "access_token": access_token_display,
        "rooms": joined,
    });

    // Python always prints JSON for --get-client-info
    println!("{}", serde_json::to_string_pretty(&info_obj).unwrap());

    Ok(())
}

/// Handle room invitations: list pending invites, join invited rooms, or both.
pub(crate) async fn room_invites(
    client: &Client,
    mode: &str,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    debug!("Room invites with mode {:?}", mode);
    let invited = client.invited_rooms();
    if invited.is_empty() {
        info!("No pending room invitations found.");
        return Ok(());
    }
    let do_list = mode == "list" || mode == "list+join";
    let do_join = mode == "join" || mode == "list+join";
    let mut err_count = 0usize;
    for room in &invited {
        let room_id = room.room_id().to_string();
        if do_list {
            match output {
                Output::Text => {
                    println!("{}{}m.room.member{}invite", room_id, sep, sep);
                }
                Output::JsonSpec => (),
                _ => {
                    println!(
                        "{{\"room_id\": \"{}\", \"event\": \"m.room.member\", \"membership\": \"invite\"}}",
                        room_id
                    );
                }
            }
        }
        if do_join {
            match room.join().await {
                Ok(_) => {
                    info!("Joined room {} successfully.", room_id);
                }
                Err(e) => {
                    error!("Error joining room {}: {}", room_id, e);
                    err_count += 1;
                }
            }
        }
    }
    if err_count > 0 {
        Err(Error::RoomInvitesFailed)
    } else {
        Ok(())
    }
}

/// Redact (remove content from) events in rooms.
/// args is a flat vector: [room_id, event_id, reason, room_id, event_id, reason, ...]
/// Also accepts exactly 2 arguments (room_id, event_id) with no reason.
pub(crate) async fn room_redact(
    client: &Client,
    args: &[String],
    _output: Output,
) -> Result<(), Error> {
    debug!("Room redact with {} arguments", args.len());
    let mut args_vec = args.to_vec();
    // If exactly 2 args, append empty reason
    if args_vec.len() == 2 {
        args_vec.push(String::new());
    }
    if args_vec.len() % 3 != 0 {
        error!(
            "Incorrect number of arguments for --room-redact. Arguments must \
            be triples (multiples of 3), but found {} arguments. 2 is also allowed.",
            args_vec.len()
        );
        return Err(Error::RoomRedactFailed);
    }
    let mut err_count = 0usize;
    let num_redactions = args_vec.len() / 3;
    for ii in 0..num_redactions {
        let room_id_str = &args_vec[ii * 3];
        let event_id_str = &args_vec[ii * 3 + 1];
        let reason_str = args_vec[ii * 3 + 2].trim().to_string();
        let reason = if reason_str.is_empty() {
            None
        } else {
            Some(reason_str.as_str())
        };
        debug!(
            "Preparing to redact event {} in room {} with reason {:?}.",
            event_id_str, room_id_str, reason
        );
        let room_id = match RoomId::parse(room_id_str) {
            Ok(id) => id,
            Err(e) => {
                error!(
                    "Error: invalid room id {:?}. Error: {}",
                    room_id_str, e
                );
                err_count += 1;
                continue;
            }
        };
        let event_id = match matrix_sdk::ruma::EventId::parse(event_id_str) {
            Ok(id) => id,
            Err(e) => {
                error!(
                    "Error: invalid event id {:?}. Error: {}",
                    event_id_str, e
                );
                err_count += 1;
                continue;
            }
        };
        let room = match client.get_room(&room_id) {
            Some(r) => r,
            None => {
                error!("Error: room {:?} not found.", room_id_str);
                err_count += 1;
                continue;
            }
        };
        match room.redact(&event_id, reason, None).await {
            Ok(_resp) => {
                info!(
                    "Successfully redacted event {} in room {} with reason '{}'.",
                    event_id_str,
                    room_id_str,
                    reason.unwrap_or("")
                );
            }
            Err(e) => {
                error!(
                    "Failed to redact event {} in room {}: {}",
                    event_id_str, room_id_str, e
                );
                err_count += 1;
            }
        }
    }
    if err_count > 0 {
        Err(Error::RoomRedactFailed)
    } else {
        Ok(())
    }
}

/// Check if the current user has specific permissions in rooms.
/// args is a flat vector of pairs: [room_id, permission_type, room_id, permission_type, ...]
/// Permission types: ban, invite, kick, notifications, redact, events_default, state_default, users_default
pub(crate) async fn has_permission(
    client: &Client,
    args: &[String],
    user_id: &OwnedUserId,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    debug!("Has permission with {} arguments", args.len());
    if args.len() % 2 != 0 {
        error!(
            "Incorrect number of arguments for --has-permission. Arguments \
            must be pairs, i.e. multiples of 2, but found {} arguments.",
            args.len()
        );
        return Err(Error::HasPermissionFailed);
    }
    let mut err_count = 0usize;
    let num_pairs = args.len() / 2;
    for ii in 0..num_pairs {
        let room_id_str = args[ii * 2].replace("\\!", "!");
        let permission_type = args[ii * 2 + 1].trim().to_string();
        debug!(
            "Preparing to ask about permission for permission type '{}' in room {}.",
            permission_type, room_id_str
        );
        let room_id = match RoomId::parse(&room_id_str) {
            Ok(id) => id,
            Err(e) => {
                error!(
                    "Error: invalid room id {:?}. Error: {}",
                    room_id_str, e
                );
                err_count += 1;
                match output {
                    Output::Text => {
                        println!(
                            "Error{}{}{}{}{}{}",
                            sep, user_id, sep, room_id_str, sep, permission_type
                        );
                    }
                    _ => (),
                }
                continue;
            }
        };
        let room = match client.get_room(&room_id) {
            Some(r) => r,
            None => {
                error!("Error: room {:?} not found.", room_id_str);
                err_count += 1;
                match output {
                    Output::Text => {
                        println!(
                            "Error{}{}{}{}{}{}",
                            sep, user_id, sep, room_id_str, sep, permission_type
                        );
                    }
                    _ => (),
                }
                continue;
            }
        };
        // Get power levels from room state
        let power_levels = match room
            .get_state_events_static::<RoomPowerLevelsEventContent>()
            .await
        {
            Ok(evs) => {
                use matrix_sdk::deserialized_responses::SyncOrStrippedState;
                use matrix_sdk::ruma::events::SyncStateEvent;
                let mut pl_content: Option<RoomPowerLevelsEventContent> = None;
                for ev in evs {
                    if let Ok(val) = ev.deserialize() {
                        match val {
                            SyncOrStrippedState::Sync(sync_ev) => {
                                if let SyncStateEvent::Original(original) = sync_ev {
                                    pl_content = Some(original.content.clone());
                                }
                            }
                            SyncOrStrippedState::Stripped(stripped_ev) => {
                                pl_content = Some(stripped_ev.content.clone());
                            }
                        }
                    }
                }
                match pl_content {
                    Some(pl) => pl,
                    None => {
                        error!(
                            "Error: could not find power levels for room {:?}.",
                            room_id_str
                        );
                        err_count += 1;
                        match output {
                            Output::Text => {
                                println!(
                                    "Error{}{}{}{}{}{}",
                                    sep, user_id, sep, room_id_str, sep, permission_type
                                );
                            }
                            _ => (),
                        }
                        continue;
                    }
                }
            }
            Err(e) => {
                error!(
                    "Error: failed to get power levels for room {:?}: {}",
                    room_id_str, e
                );
                err_count += 1;
                match output {
                    Output::Text => {
                        println!(
                            "Error{}{}{}{}{}{}",
                            sep, user_id, sep, room_id_str, sep, permission_type
                        );
                    }
                    _ => (),
                }
                continue;
            }
        };
        // Get the user's power level
        let user_power_level: i64 = {
            let upl = power_levels.users.get(user_id);
            match upl {
                Some(pl) => i64::from(*pl),
                None => i64::from(power_levels.users_default),
            }
        };
        // Get the required power level for the permission type
        let required_level: i64 = match permission_type.as_str() {
            "ban" => i64::from(power_levels.ban),
            "invite" => i64::from(power_levels.invite),
            "kick" => i64::from(power_levels.kick),
            "redact" => i64::from(power_levels.redact),
            "notifications" => {
                // The notifications power level for @room mentions
                i64::from(power_levels.notifications.room)
            }
            "events_default" => i64::from(power_levels.events_default),
            "state_default" => i64::from(power_levels.state_default),
            "users_default" => i64::from(power_levels.users_default),
            other => {
                // Try to look up as a specific event type in events map
                use matrix_sdk::ruma::events::TimelineEventType;
                let evt = TimelineEventType::from(other.to_string());
                match power_levels.events.get(&evt) {
                    Some(level) => i64::from(*level),
                    None => i64::from(power_levels.events_default),
                }
            }
        };
        let has_perm = user_power_level >= required_level;
        debug!(
            "has_permission {} for permission type '{}' in room {}: {} (user_level={}, required={})",
            user_id, permission_type, room_id_str, has_perm, user_power_level, required_level
        );
        match output {
            Output::Text => {
                // Python format: resp{SEP}user_id{SEP}room_id{SEP}permission_type
                // where resp is True/False (Python bool representation)
                println!(
                    "{}{}{}{}{}{}{}",
                    if has_perm { "True" } else { "False" }, sep, user_id, sep, room_id_str, sep, permission_type
                );
            }
            _ => {
                let json_obj = serde_json::json!({
                    "has_permission": has_perm,
                    "user_id": user_id.to_string(),
                    "room_id": room_id_str,
                    "permission_type": permission_type,
                    "user_power_level": user_power_level,
                    "required_power_level": required_level,
                });
                println!("{}", serde_json::to_string(&json_obj).unwrap());
            }
        }
    }
    if err_count > 0 {
        Err(Error::HasPermissionFailed)
    } else {
        Ok(())
    }
}

/// Get joined DM rooms for specified users.
/// If '*' is given, list all DM rooms.
pub(crate) async fn joined_dm_rooms(
    client: &Client,
    users: &[String],
    user_id: &OwnedUserId,
    output: Output,
    sep: &str,
) -> Result<(), Error> {
    debug!("Joined DM rooms for users: {:?}", users);
    if users.is_empty() {
        warn!("No users specified for --joined-dm-rooms. Nothing to do.");
        return Ok(());
    }
    let all_users = users.iter().any(|u| u.trim() == "*");
    let user_set: std::collections::HashSet<&str> =
        users.iter().map(|u| u.trim()).collect();

    // Get all joined rooms
    let joined = client.joined_rooms();
    let mut err_count = 0usize;

    // Build a map of receiver_user_id -> Vec<{room_id, members}>
    let mut users_dict: std::collections::BTreeMap<
        String,
        Vec<(String, Vec<(String, String, String)>)>,
    > = std::collections::BTreeMap::new();

    for room in &joined {
        let room_id = room.room_id().to_string();
        let members = match room.members(RoomMemberships::JOIN).await {
            Ok(m) => m,
            Err(e) => {
                error!(
                    "Error: failed to get members of room {:?}: {}",
                    room_id, e
                );
                err_count += 1;
                continue;
            }
        };
        // DM rooms have exactly 2 members
        if members.len() == 2 {
            let (sender_idx, receiver_idx) = if members[0].user_id() == user_id {
                (0usize, 1usize)
            } else if members[1].user_id() == user_id {
                (1usize, 0usize)
            } else {
                error!(
                    "Error: sender does not match in room {:?}",
                    room_id
                );
                err_count += 1;
                continue;
            };
            let rcvr_id = members[receiver_idx].user_id().to_string();
            // Check if this user is requested
            if all_users || user_set.contains(rcvr_id.as_str()) {
                let member_data: Vec<(String, String, String)> = members
                    .iter()
                    .map(|m| {
                        (
                            m.user_id().to_string(),
                            m.display_name().unwrap_or("").to_string(),
                            m.avatar_url()
                                .map(|u| u.to_string())
                                .unwrap_or_default(),
                        )
                    })
                    .collect();
                let _ = sender_idx; // suppress unused warning
                users_dict
                    .entry(rcvr_id)
                    .or_default()
                    .push((room_id, member_data));
            }
        }
    }

    // Print results matching Python format
    for (user, rooms) in &users_dict {
        for (room_id, members) in rooms {
            match output {
                Output::Text => {
                    // Python format: user{SEP}room_id{SEP}member1_id{SEP}member1_displayname{SEP}member1_avatar{SEP}member2_id...
                    let mut text = format!("{}{}{}", user, sep, room_id);
                    for (mid, dname, avatar) in members {
                        text.push_str(&format!(
                            "{}{}{}{}{}{}",
                            sep, mid, sep, dname, sep, avatar
                        ));
                    }
                    println!("{}", text.trim_end());
                }
                _ => {
                    let members_json: Vec<serde_json::Value> = members
                        .iter()
                        .map(|(mid, dname, avatar)| {
                            serde_json::json!({
                                "user_id": mid,
                                "display_name": dname,
                                "avatar_url": avatar,
                            })
                        })
                        .collect();
                    let json_obj = serde_json::json!({
                        "user_id": user,
                        "room_id": room_id,
                        "members": members_json,
                    });
                    println!("{}", serde_json::to_string(&json_obj).unwrap());
                }
            }
        }
    }
    if err_count > 0 {
        Err(Error::JoinedDmRoomsFailed)
    } else {
        Ok(())
    }
}

/// Invoke raw Matrix REST API calls.
/// args is a flat vector of triples: [method, data, url, method, data, url, ...]
pub(crate) async fn rest(
    _client: &Client,
    args: &[String],
    credentials: &Credentials,
    access_token_override: Option<&str>,
    output: Output,
) -> Result<(), Error> {
    debug!("REST API call with {} arguments", args.len());
    if args.len() % 3 != 0 {
        error!(
            "Incorrect number of arguments for --rest. Arguments must \
            be triples, i.e. multiples of 3, but found {} arguments.",
            args.len()
        );
        return Err(Error::RestFailed);
    }
    let mut err_count = 0usize;
    let num_calls = args.len() / 3;
    // Use the matrix-sdk's underlying http client for raw API calls
    // We build our own reqwest client for this purpose
    let http_client = reqwest::Client::new();

    let at = match access_token_override {
        Some(t) => t.to_string(),
        None => credentials.access_token.clone(),
    };
    let homeserver = {
        let mut hs = credentials.homeserver.to_string();
        if hs.ends_with('/') {
            hs.pop();
        }
        hs
    };
    let hostname = credentials
        .homeserver
        .host_str()
        .unwrap_or("")
        .to_string();
    let user_id_str = credentials.user_id.to_string();
    let device_id_str = credentials.device_id.to_string();
    let room_id_str = credentials.room_id.clone();

    for ii in 0..num_calls {
        let method_str = args[ii * 3].to_uppercase().trim().to_string();
        let mut data = args[ii * 3 + 1].clone();
        let mut url = args[ii * 3 + 2].clone();

        // Validate method
        if !["GET", "POST", "PUT", "DELETE", "OPTIONS"].contains(&method_str.as_str()) {
            error!(
                "Incorrect REST method {:?}. Must be one of: GET, POST, PUT, DELETE, OPTIONS.",
                method_str
            );
            err_count += 1;
            continue;
        }
        if url.trim().is_empty() {
            error!("Incorrect REST URL. Must not be empty.");
            err_count += 1;
            continue;
        }

        // Replace placeholders
        let encoded_room_id = urlencoding::encode(&room_id_str);
        for (placeholder, value) in &[
            ("__homeserver__", homeserver.as_str()),
            ("__hostname__", hostname.as_str()),
            ("__access_token__", at.as_str()),
            ("__user_id__", user_id_str.as_str()),
            ("__device_id__", device_id_str.as_str()),
            ("__room_id__", encoded_room_id.as_ref()),
        ] {
            data = data.replace(placeholder, value);
            url = url.replace(placeholder, value);
        }
        url = url.trim().to_string();

        // Redact access token from logged data and URL
        let redacted_data = data.replace(at.as_str(), &redact_token(&at));
        let redacted_url = url.replace(at.as_str(), &redact_token(&at));

        if !data.is_empty() && ["GET", "DELETE", "OPTIONS"].contains(&method_str.as_str()) {
            warn!(
                "Found REST data {:?} for method {}. \
                There is usually no data for GET, DELETE, OPTIONS. \
                Most likely this is not what you want.",
                redacted_data, method_str
            );
            err_count += 1;
            continue;
        }
        debug!(
            "Preparing to invoke REST API call: method={} data={}, url={}.",
            method_str, redacted_data, redacted_url
        );

        let request = match method_str.as_str() {
            "GET" => http_client.get(&url),
            "POST" => http_client.post(&url).body(data.clone()),
            "PUT" => http_client.put(&url).body(data.clone()),
            "DELETE" => http_client.delete(&url),
            "OPTIONS" => http_client.request(reqwest::Method::OPTIONS, &url),
            _ => unreachable!(),
        };

        match request.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let txt = resp.text().await.unwrap_or_default();
                if status != 200 {
                    error!(
                        "REST API call failed. Error code {} and error text {}. \
                        Input: method={} data={}, url={}.",
                        status, txt, method_str, redacted_data, redacted_url
                    );
                    err_count += 1;
                } else {
                    debug!(
                        "REST API call successful. Response: {}. Input: method={} data={}, url={}.",
                        txt, method_str, redacted_data, redacted_url
                    );
                    match output {
                        Output::Text => {
                            println!("{}", txt);
                        }
                        _ => {
                            // Try to parse the response as JSON, fallback to string
                            let json_val: serde_json::Value =
                                serde_json::from_str(&txt).unwrap_or_else(|_| {
                                    serde_json::json!({ "response": txt })
                                });
                            println!("{}", serde_json::to_string(&json_val).unwrap());
                        }
                    }
                }
            }
            Err(e) => {
                error!(
                    "REST API call failed with error: {}. Input: method={} data={}, url={}.",
                    e, method_str, redacted_data, redacted_url
                );
                err_count += 1;
            }
        }
    }
    if err_count > 0 {
        Err(Error::RestFailed)
    } else {
        Ok(())
    }
}

/// Parse a key dictionary string (Python-style JSON) into a matrix-sdk EncryptedFile.
/// The key dict is used with --key-dict for decrypting encrypted media downloads.
fn parse_key_dict_to_encrypted_file(
    mxc_uri: &OwnedMxcUri,
    key_dict_str: &str,
) -> Result<matrix_sdk::ruma::events::room::EncryptedFile, Error> {
    use matrix_sdk::ruma::events::room::{EncryptedFileInit, JsonWebKey, JsonWebKeyInit};
    use matrix_sdk::ruma::serde::{Base64, base64::UrlSafe};

    // Normalize Python-style JSON: single quotes -> double quotes, True/False -> true/false
    let normalized = key_dict_str
        .replace('\'', "\"")
        .replace("True", "true")
        .replace("False", "false");
    let dict: serde_json::Value = serde_json::from_str(&normalized).map_err(|e| {
        error!("Failed to parse key dictionary: {}", e);
        Error::MediaDownloadFailed
    })?;

    let key_obj = &dict["key"];
    let key_k = key_obj["k"]
        .as_str()
        .ok_or_else(|| {
            error!("Missing 'key.k' in key dictionary");
            Error::MediaDownloadFailed
        })?;
    let key_alg = key_obj["alg"].as_str().unwrap_or("A256CTR");
    let key_ext = key_obj["ext"].as_bool().unwrap_or(true);
    let key_kty = key_obj["kty"].as_str().unwrap_or("oct");
    let key_ops: Vec<String> = key_obj["key_ops"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        })
        .unwrap_or_else(|| vec!["encrypt".to_owned(), "decrypt".to_owned()]);

    let sha256 = dict["hashes"]["sha256"]
        .as_str()
        .ok_or_else(|| {
            error!("Missing 'hashes.sha256' in key dictionary");
            Error::MediaDownloadFailed
        })?;
    let iv_str = dict["iv"].as_str().ok_or_else(|| {
        error!("Missing 'iv' in key dictionary");
        Error::MediaDownloadFailed
    })?;

    let jwk = JsonWebKey::from(JsonWebKeyInit {
        kty: key_kty.to_owned(),
        key_ops,
        alg: key_alg.to_owned(),
        k: Base64::<UrlSafe>::parse(key_k).map_err(|_| {
            error!("Failed to parse Base64 key 'k'");
            Error::MediaDownloadFailed
        })?,
        ext: key_ext,
    });

    let mut hashes = std::collections::BTreeMap::new();
    hashes.insert(
        "sha256".to_owned(),
        Base64::parse(sha256).map_err(|_| {
            error!("Failed to parse Base64 hash 'sha256'");
            Error::MediaDownloadFailed
        })?,
    );

    let init = EncryptedFileInit {
        url: mxc_uri.clone(),
        key: jwk,
        iv: Base64::parse(iv_str).map_err(|_| {
            error!("Failed to parse Base64 'iv'");
            Error::MediaDownloadFailed
        })?,
        hashes,
        v: dict["v"].as_str().unwrap_or("v2").to_owned(),
    };
    Ok(init.into())
}
