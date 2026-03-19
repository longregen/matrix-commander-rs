// cli.rs — CLI handler functions and helper utilities

use matrix_sdk::Client;
use rpassword::read_password;
use std::io::{self, stdin, IsTerminal, Read, Write};
use std::path::PathBuf;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::args::Args;
use matrix_sdk::ruma::{api::client::room::Visibility, OwnedUserId};
use crate::listen::{listen_all, listen_forever, listen_once, listen_tail};
use crate::mclient::{
    bootstrap, content_repository_config, convert_to_full_room_id, delete_devices_pre,
    delete_mxc_before, devices, discovery_info, event, export_keys, file, get_avatar,
    get_avatar_url, get_client_info, get_display_name, get_masterkey, get_openid_token,
    get_presence, get_profile, get_room_info, has_permission, import_keys, invited_rooms,
    joined_dm_rooms, joined_members, joined_rooms, left_rooms, login, login_access_token, login_sso,
    login_info, logout, media_delete, media_download, media_mxc_to_http, media_upload, message,
    rest, restore_login, room_ban, room_create, room_delete_alias, room_enable_encryption,
    room_forget, room_get_state, room_get_visibility, room_invite, room_join, room_kick,
    room_leave, room_redact, room_resolve_alias, room_set_alias, room_unban, rooms, set_avatar,
    set_avatar_url, set_device_name, set_display_name, set_presence, unset_avatar_url, verify,
};
pub(crate) use crate::mclient::restore_credentials;
use crate::types::*;

use crate::get_prog_without_ext;
use crate::get_store_default_path;

/// Gets the *actual* path (including file name) of the credentials file
/// The default path might not be the actual path as it can be overwritten with command line
/// options.
fn get_credentials_actual_path(ap: &Args) -> &PathBuf {
    &ap.credentials
}

/// Return true if credentials file exists, false otherwise
pub(crate) fn credentials_exist(ap: &Args) -> bool {
    let ap = get_credentials_actual_path(ap);
    debug!("credentials_actual_path = {:?}", ap);
    let exists = ap.is_file();
    if exists {
        debug!("{:?} exists and is file. Not sure if readable though.", ap);
    } else {
        debug!("{:?} does not exist or is not a file.", ap);
    }
    exists
}

/// Gets the *actual* path (including file name) of the store directory
/// The default path might not be the actual path as it can be overwritten with command line
/// options.
/// set_store() must be called before this function is ever called.
fn get_store_actual_path(ap: &Args) -> &PathBuf {
    &ap.store
}

/// Return true if store dir exists, false otherwise
#[allow(dead_code)]
fn store_exist(ap: &Args) -> bool {
    let dp = get_store_default_path();
    let ap = get_store_actual_path(ap);
    debug!(
        "store_default_path = {:?}, store_actual_path = {:?}",
        dp, ap
    );
    let exists = ap.is_dir();
    if exists {
        debug!(
            "{:?} exists and is directory. Not sure if readable though.",
            ap
        );
    } else {
        debug!("{:?} does not exist or is not a directory.", ap);
    }
    exists
}


/// If necessary reads homeserver name for login and puts it into the Args.
/// If already set via --homeserver option, then it does nothing.
fn get_homeserver(ap: &mut Args) {
    while ap.homeserver.is_none() {
        print!("Enter your Matrix homeserver (e.g. https://some.homeserver.org): ");
        if let Err(e) = io::stdout().flush() {
            warn!("Warning: Failed to flush stdout: {e}");
        }
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            error!("Error: Unable to read user input");
            continue; // Skip to the next iteration if reading input fails
        }
        let trimmed_input = input.trim();
        if trimmed_input.is_empty() {
            error!("Error: Empty homeserver name is not allowed!");
        } else {
            match Url::parse(trimmed_input) {
                Ok(url) => {
                    ap.homeserver = Some(url);
                }
                Err(e) => {
                    error!(
                        "Error: The syntax is incorrect. Homeserver must be a valid URL! \
                        Start with 'http://' or 'https://'. Details: {e}"
                    );
                    continue;
                }
            }
            debug!("homeserver is {}", ap.homeserver.as_ref().unwrap());
        }
    }
}

/// If necessary reads user name for login and puts it into the Args.
/// If already set via --user-login option, then it does nothing.
fn get_user_login(ap: &mut Args) {
    while ap.user_login.is_none() {
        print!("Enter your full Matrix username (e.g. @john:some.homeserver.org): ");
        if let Err(e) = io::stdout().flush() {
            warn!("Warning: Failed to flush stdout: {e}");
        }
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            error!("Error: Unable to read user input");
            continue; // Skip to the next iteration if reading input fails
        }
        let trimmed_input = input.trim();
        if trimmed_input.is_empty() {
            error!("Error: Empty username is not allowed!");
        } else if !is_valid_username(trimmed_input) {
            error!("Error: Invalid username format!");
        } else {
            ap.user_login = Some(trimmed_input.to_string());
            debug!("user_login is {trimmed_input}");
        }
    }
}

// validation function for username format
fn is_valid_username(username: &str) -> bool {
    // Check if it starts with '@', contains ':', etc.
    username.starts_with('@') && username.contains(':')
}

/// If necessary reads password for login and puts it into the Args.
/// If already set via --password option, then it does nothing.
pub(crate) fn get_password(ap: &mut Args) {
    while ap.password.is_none() {
        print!("Enter Matrix password for this user: ");
        // Flush stdout to ensure the prompt is displayed
        if let Err(e) = io::stdout().flush() {
            warn!("Warning: Failed to flush stdout: {e}");
        }
        // Handle potential errors from read_password
        match read_password() {
            Ok(password) => {
                let trimmed_password = password.trim();
                if trimmed_password.is_empty() {
                    error!("Error: Empty password is not allowed!");
                } else {
                    ap.password = Some(password);
                    // Hide password from debug log files
                    debug!("password is {}", "******");
                }
            }
            Err(e) => {
                error!("Error reading password: {e}");
            }
        }
    }
}

/// If necessary reads device for login and puts it into the Args.
/// If already set via --device option, then it does nothing.
fn get_device(ap: &mut Args) {
    while ap.device.is_none() {
        print!(
            "Enter your desired name for the Matrix device that \
            is going to be created for you (e.g. {}): ",
            get_prog_without_ext()
        );
        if let Err(e) = io::stdout().flush() {
            warn!("Warning: Failed to flush stdout: {e}");
        }
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            error!("Error: Unable to read user input");
            continue; // Skip to the next iteration if reading input fails
        }
        let trimmed_input = input.trim();
        if trimmed_input.is_empty() {
            error!("Error: Empty device name is not allowed!");
        } else {
            ap.device = Some(trimmed_input.to_string());
            debug!("device is {trimmed_input}");
        }
    }
}

/// If necessary reads room_default for login and puts it into the Args.
/// If already set via --room_default option, then it does nothing.
fn get_room_default(ap: &mut Args) {
    while ap.room_default.is_none() {
        print!(
            "Enter name of one of your Matrix rooms that you want to use as default room  \
            (e.g. !someRoomId:some.homeserver.org): "
        );
        if let Err(e) = io::stdout().flush() {
            warn!("Warning: Failed to flush stdout: {e}");
        }
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            error!("Error: Unable to read user input");
            continue; // Skip to the next iteration if reading input fails
        }
        let trimmed_input = input.trim();
        if trimmed_input.is_empty() {
            error!("Error: Empty name of default room is not allowed!");
        } else if !is_valid_room_name(trimmed_input) {
            error!("Error: Invalid room name format for '{trimmed_input}'! Room name must start with '!' and contain exactly one ':'.");
        } else {
            ap.room_default = Some(trimmed_input.to_string());
            debug!("room_default is '{trimmed_input}'");
        }
    }
}

// Validation function for room name format
fn is_valid_room_name(name: &str) -> bool {
    name.starts_with('!') && name.matches(':').count() == 1
}

/// A room is either specified with --room or the default from credentials file is used
/// On error return None.
pub(crate) fn set_rooms(ap: &mut Args, default_room: &str) {
    debug!("set_rooms()");
    if ap.room.is_empty() {
        ap.room.push(default_room.to_string()); // since --room is empty, use default room from credentials
    }
}

// /// Before get_rooms() is called the rooms should have been updated with set_rooms() first.
// /// Get the user specified rooms (which might either have been specified with --room or
// /// be the default room from the credentials file).
// /// On error return None.
// fn get_rooms(ap: &Args) -> &Vec<String> {
//     debug!("get_rooms()");
//     &ap.room
// }

/// Get the default room id from the credentials file.
/// On error return None.
pub(crate) async fn get_room_default_from_credentials(client: &Client, credentials: &Credentials) -> String {
    let mut room = credentials.room_id.clone();
    convert_to_full_room_id(
        client,
        &mut room,
        credentials.homeserver.host_str().unwrap(),
    )
    .await;
    room
}

/// A user is either specified with --user or the default from credentials file is used
/// On error return None.
pub(crate) fn set_users(ap: &mut Args) -> Result<(), Error> {
    debug!("set_users()");
    if ap.user.is_empty() {
        let duser = get_user_default_from_credentials(ap.creds.as_ref().ok_or(Error::NotLoggedIn)?);
        ap.user.push(duser.to_string()); // since --user is empty, use default user from credentials
    }
    Ok(())
}

/// Before get_users() is called the users should have been updated with set_users() first.
/// Get the user specified users (which might either have been specified with --user or
/// be the default user from the credentials file).
/// On error return None.
#[allow(dead_code)]
fn get_users(ap: &Args) -> &Vec<String> {
    debug!("get_users()");
    &ap.user
}

/// Get the default user id from the credentials file.
/// On error return None.
fn get_user_default_from_credentials(credentials: &Credentials) -> OwnedUserId {
    credentials.user_id.clone()
}

/// Convert a vector of aliases that can contain short alias forms into
/// a vector of fully canonical aliases.
/// john and #john will be converted to #john:matrix.server.org.
/// vecstr: the vector of aliases
/// default_host: the default hostname like "matrix.server.org"
pub(crate) fn convert_to_full_room_aliases(vecstr: &mut Vec<String>, default_host: &str) {
    vecstr.retain(|x| !x.trim().is_empty());
    for el in vecstr {
        el.retain(|c| !c.is_whitespace());
        if el.starts_with('!') {
            warn!("A room id was given as alias. {:?}", el);
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

// Replace shortcut "-" with room id of default room
pub(crate) fn replace_minus_with_default_room(vecstr: &mut Vec<String>, default_room: &str) {
    // There is no way to distringuish --get-room-info not being in CLI
    // and --get-room-info being in API without a room.
    // Hence it is not possible to say "if vector is empty let's use the default room".
    // The user has to specify something, we used "-".
    if vecstr.iter().any(|x| x.trim() == "-") {
        vecstr.push(default_room.to_string());
    }
    vecstr.retain(|x| x.trim() != "-");
}

/// Handle the --login CLI argument
pub(crate) async fn cli_login(ap: &mut Args) -> Result<(Client, Credentials), Error> {
    if ap.login.is_none() {
        return Err(Error::UnsupportedCliParameter("--login cannot be empty"));
    }
    if credentials_exist(ap) {
        error!(concat!(
            "Credentials file already exists. You have already logged in in ",
            "the past. No login needed. Skipping login. If you really want to log in ",
            "(i.e. create a new device), then logout first, or move credentials file manually. ",
            "Or just run your command again but without the '--login' option to log in ",
            "via your existing credentials and access token. ",
        ));
        return Err(Error::LoginUnnecessary);
    }
    if ap.login == Login::Sso {
        // SSO login: requires --homeserver, device name optional
        get_homeserver(ap);
        get_device(ap);
        get_room_default(ap);
        info!(
            "Parameters for SSO login are: {:?} {:?} {:?}",
            ap.homeserver, ap.device, ap.room_default,
        );
        let (client, credentials) = login_sso(
            ap,
            &ap.homeserver.clone().ok_or(Error::MissingCliParameter)?,
            &ap.device.clone().ok_or(Error::MissingCliParameter)?,
            &ap.room_default.clone().ok_or(Error::MissingCliParameter)?,
        )
        .await?;
        return Ok((client, credentials));
    }
    if !ap.login.is_password() && !ap.login.is_access_token() {
        error!(
            "Login option '{:?}' currently not supported. Use '{:?}', '{:?}', or '{:?}'.",
            ap.login,
            Login::Password,
            Login::AccessToken,
            Login::Sso,
        );
        return Err(Error::UnsupportedCliParameter(
            "Used login option currently not supported. Use 'password', 'access-token', or 'sso'.",
        ));
    }
    if ap.login.is_access_token() {
        // login with access token: requires --homeserver, --user-login, --access-token
        get_homeserver(ap);
        get_user_login(ap);
        if ap.access_token.is_none() {
            error!("--login access-token requires --access-token to be provided.");
            return Err(Error::MissingCliParameter);
        }
        get_room_default(ap);
        let device_name = ap.device.clone().unwrap_or_else(|| crate::get_prog_without_ext().to_string());
        info!(
            "Parameters for access-token login are: {:?} {:?} {:?}",
            ap.homeserver, ap.user_login, ap.room_default,
        );
        let (client, credentials) = login_access_token(
            ap,
            &ap.homeserver.clone().ok_or(Error::MissingCliParameter)?,
            &ap.user_login.clone().ok_or(Error::MissingCliParameter)?,
            &ap.access_token.clone().ok_or(Error::MissingCliParameter)?,
            &device_name,
            &ap.room_default.clone().ok_or(Error::MissingCliParameter)?,
        )
        .await?;
        return Ok((client, credentials));
    }
    // login is Login::Password
    get_homeserver(ap);
    get_user_login(ap);
    get_password(ap);
    get_device(ap); // human-readable device name
    get_room_default(ap);
    // hide password from debug log file // ap.password
    info!(
        "Parameters for login are: {:?} {:?} {:?} {:?} {:?}",
        ap.homeserver, ap.user_login, "******", ap.device, ap.room_default
    );
    let (client, credentials) = login(
        ap,
        &ap.homeserver.clone().ok_or(Error::MissingCliParameter)?,
        &ap.user_login.clone().ok_or(Error::MissingCliParameter)?,
        &ap.password.clone().ok_or(Error::MissingCliParameter)?,
        &ap.device.clone().ok_or(Error::MissingCliParameter)?,
        &ap.room_default.clone().ok_or(Error::MissingCliParameter)?,
    )
    .await?;
    Ok((client, credentials))
}

/// Attempt a restore-login iff the --login CLI argument is missing.
/// In other words try a re-login using the access token from the credentials file.
pub(crate) async fn cli_restore_login(
    credentials: &Credentials,
    ap: &Args,
    needs_sync: bool,
) -> Result<Client, Error> {
    info!("restore_login implicitly chosen.");
    restore_login(credentials, ap, needs_sync).await
}

/// Handle the --bootstrap CLI argument
pub(crate) async fn cli_bootstrap(client: &Client, ap: &mut Args) -> Result<(), Error> {
    info!("Bootstrap chosen.");
    bootstrap(client, ap).await
}

/// Handle the --verify CLI argument
pub(crate) async fn cli_verify(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Verify chosen.");
    if ap.verify.is_none() {
        return Err(Error::UnsupportedCliParameter(
            "Argument --verify cannot be empty",
        ));
    }
    if !ap.verify.is_manual_device()
        && !ap.verify.is_manual_user()
        && !ap.verify.is_emoji()
        && !ap.verify.is_emoji_req()
    {
        error!(
            "Verify option '{:?}' currently not supported. \
            Use '{:?}', '{:?}', '{:?}' or {:?}' for the time being.",
            ap.verify,
            Verify::ManualDevice,
            Verify::ManualUser,
            Verify::Emoji,
            Verify::EmojiReq
        );
        return Err(Error::UnsupportedCliParameter(
            "Used --verify option is currently not supported",
        ));
    }
    verify(client, ap).await
}


/// Handle the --message CLI argument
pub(crate) async fn cli_message(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Message chosen.");
    if ap.message.is_empty() {
        return Ok(()); // nothing to do
    }
    let mut fmsgs: Vec<String> = Vec::new(); // formatted msgs
    for msg in ap.message.iter() {
        if msg.is_empty() {
            info!("Skipping empty text message.");
            continue;
        };
        if msg == "--" {
            info!("Skipping '--' text message as these are used to separate arguments.");
            continue;
        };
        // - map to - (stdin pipe)
        // \- maps to text r'-', a 1-letter message
        let fmsg = if msg == r"-" {
            let mut line = String::new();
            if stdin().is_terminal() {
                print!("Message: ");
                io::stdout().flush()?;
                io::stdin().read_line(&mut line)?;
            } else {
                io::stdin().read_to_string(&mut line)?;
            }
            // line.trim_end().to_string() // remove /n at end of string
            line.strip_suffix("\r\n")
                .or(line.strip_suffix("\n"))
                .unwrap_or(&line)
                .to_string() // remove /n at end of string
        } else if msg == r"_" {
            let mut eof = false;
            while !eof {
                let mut line = String::new();
                match io::stdin().read_line(&mut line) {
                    // If this function returns Ok(0), the stream has reached EOF.
                    Ok(n) => {
                        if n == 0 {
                            eof = true;
                            debug!("Reached EOF of pipe stream.");
                        } else {
                            debug!(
                                "Read {n} bytes containing \"{}\\n\" from pipe stream.",
                                line.trim_end()
                            );
                            match message(
                                client,
                                &[line],
                                &ap.room,
                                ap.code,
                                ap.markdown,
                                ap.notice,
                                ap.emote,
                                ap.html,
                                ap.print_event_id,
                                ap.output,
                                &ap.separator,
                            )
                            .await
                            {
                                Ok(()) => {
                                    debug!("message from pipe stream sent successfully");
                                }
                                Err(ref e) => {
                                    error!(
                                        "Error: sending message from pipe stream reported {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(ref e) => {
                        error!("Error: reading from pipe stream reported {}", e);
                    }
                }
            }
            "".to_owned()
        } else if msg == r"\-" {
            "-".to_string()
        } else if msg == r"\_" {
            "_".to_string()
        } else if msg == r"\-\-" {
            "--".to_string()
        } else if msg == r"\-\-\-" {
            "---".to_string()
        } else {
            msg.to_string()
        };
        if !fmsg.is_empty() {
            fmsgs.push(fmsg);
        }
    }
    if fmsgs.is_empty() {
        return Ok(()); // nothing to do
    }
    // Handle --split: split messages by separator
    if let Some(ref sep) = ap.split {
        let sep_decoded = sep
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\\", "\\");
        let mut split_msgs: Vec<String> = Vec::new();
        for fmsg in &fmsgs {
            for part in fmsg.split(&sep_decoded) {
                let trimmed = part.to_string();
                if !trimmed.is_empty() {
                    split_msgs.push(trimmed);
                }
            }
        }
        fmsgs = split_msgs;
    }
    // Handle --emojize: convert :shortcodes: to emoji
    if ap.emojize {
        fmsgs = fmsgs.iter().map(|msg| emojize_message(msg)).collect();
    }
    message(
        client,
        &fmsgs,
        &ap.room,
        ap.code,
        ap.markdown,
        ap.notice,
        ap.emote,
        ap.html,
        ap.print_event_id,
        ap.output,
        &ap.separator,
    )
    .await // returning
}

/// Convert emoji shortcodes like :thumbs_up: to actual emoji characters
fn emojize_message(msg: &str) -> String {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r":([a-zA-Z0-9_+-]+):").unwrap());
    RE.replace_all(msg, |caps: &regex::Captures| {
        let shortcode = &caps[1];
        match emojis::get_by_shortcode(shortcode) {
            Some(emoji) => emoji.as_str().to_string(),
            None => caps[0].to_string(), // keep original if not found
        }
    })
    .to_string()
}

/// Handle the --file CLI argument
pub(crate) async fn cli_file(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("File chosen.");
    if ap.file.is_empty() {
        return Ok(()); // nothing to do
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for filename in &ap.file {
        match filename.as_str() {
            "" => info!("Skipping empty file name."),
            r"-" => files.push(PathBuf::from("-".to_string())),
            r"\-" => files.push(PathBuf::from(r"\-".to_string())),
            _ => files.push(PathBuf::from(filename)),
        }
    }
    // pb: label to attach to a stdin pipe data in case there is data piped in from stdin
    let pb: PathBuf = if !ap.file_name.is_empty() {
        ap.file_name[0].clone()
    } else {
        PathBuf::from("file")
    };
    file(
        client, &files, &ap.room, None, // label, use default filename
        None, // mime, guess it
        &pb,  // label for stdin pipe
    )
    .await // returning
}

/// Handle the --image CLI argument
pub(crate) async fn cli_image(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Image chosen.");
    if ap.image.is_empty() {
        return Ok(());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for filename in &ap.image {
        match filename.as_str() {
            "" => info!("Skipping empty image file name."),
            r"-" => files.push(PathBuf::from("-".to_string())),
            r"\-" => files.push(PathBuf::from(r"\-".to_string())),
            _ => files.push(PathBuf::from(filename)),
        }
    }
    let pb: PathBuf = if !ap.file_name.is_empty() {
        ap.file_name[0].clone()
    } else {
        PathBuf::from("image.png")
    };
    // Force image mime type if not guessable
    file(
        client, &files, &ap.room, None,
        None, // mime will be guessed from filename; send_attachment sets m.image for image/* mimes
        &pb,
    )
    .await
}

/// Handle the --audio CLI argument
pub(crate) async fn cli_audio(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Audio chosen.");
    if ap.audio.is_empty() {
        return Ok(());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for filename in &ap.audio {
        match filename.as_str() {
            "" => info!("Skipping empty audio file name."),
            r"-" => files.push(PathBuf::from("-".to_string())),
            r"\-" => files.push(PathBuf::from(r"\-".to_string())),
            _ => files.push(PathBuf::from(filename)),
        }
    }
    let pb: PathBuf = if !ap.file_name.is_empty() {
        ap.file_name[0].clone()
    } else {
        PathBuf::from("audio.ogg")
    };
    file(
        client, &files, &ap.room, None,
        None,
        &pb,
    )
    .await
}

/// Handle the --event CLI argument
pub(crate) async fn cli_event(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Event chosen.");
    if ap.event.is_empty() {
        return Ok(());
    }
    let mut events: Vec<String> = Vec::new();
    for ev in ap.event.iter() {
        if ev.is_empty() {
            info!("Skipping empty event.");
            continue;
        }
        if ev == r"-" {
            let mut line = String::new();
            if stdin().is_terminal() {
                print!("Event JSON: ");
                io::stdout().flush()?;
                io::stdin().read_line(&mut line)?;
            } else {
                io::stdin().read_to_string(&mut line)?;
            }
            let line = line.strip_suffix("\r\n")
                .or(line.strip_suffix("\n"))
                .unwrap_or(&line)
                .to_string();
            if !line.is_empty() {
                events.push(line);
            }
        } else {
            events.push(ev.to_string());
        }
    }
    if events.is_empty() {
        return Ok(());
    }
    event(client, &events, &ap.room).await
}

/// Handle the --import-keys CLI argument
pub(crate) async fn cli_import_keys(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Import-keys chosen.");
    if ap.import_keys.len() != 2 {
        return Err(Error::MissingCliParameter);
    }
    import_keys(client, &ap.import_keys[0], &ap.import_keys[1], ap.output).await
}

/// Handle the --export-keys CLI argument
pub(crate) async fn cli_export_keys(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Export-keys chosen.");
    if ap.export_keys.len() != 2 {
        return Err(Error::MissingCliParameter);
    }
    export_keys(client, &ap.export_keys[0], &ap.export_keys[1], ap.output).await
}

/// Handle the --get-openid-token CLI argument
pub(crate) async fn cli_get_openid_token(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-openid-token chosen.");
    get_openid_token(client, ap.output, &ap.separator).await
}

/// Handle the --media-upload CLI argument
pub(crate) async fn cli_media_upload(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Media upload chosen.");
    media_upload(client, &ap.media_upload, &ap.mime, ap.output, &ap.separator).await // returning
}

/// Handle the --media-download once CLI argument
pub(crate) async fn cli_media_download(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Media download chosen.");
    media_download(client, &ap.media_download, &ap.file_name, &ap.key_dict, ap.output).await // returning
}

/// Handle the --media-delete once CLI argument
pub(crate) async fn cli_media_delete(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Media delete chosen.");
    media_delete(client, &ap.media_delete, ap.output).await // returning
}

/// Handle the --media-mxc-to-http once CLI argument
pub(crate) async fn cli_media_mxc_to_http(ap: &Args) -> Result<(), Error> {
    info!("Media mxc_to_http chosen.");
    media_mxc_to_http(
        &ap.media_mxc_to_http,
        &ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.homeserver,
        ap.output,
        &ap.separator,
    )
    .await // returning
}

/// Handle the --listen once CLI argument
pub(crate) async fn cli_listen_once(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Listen Once chosen.");
    listen_once(client, ap.listen_self, whoami(ap)?, ap.output, ap.print_event_id).await // returning
}

/// Handle the --listen forever CLI argument
pub(crate) async fn cli_listen_forever(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Listen Forever chosen.");
    listen_forever(client, ap.listen_self, whoami(ap)?, ap.output, ap.print_event_id).await
    // returning
}

/// Handle the --listen tail CLI argument
pub(crate) async fn cli_listen_tail(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Listen Tail chosen.");
    listen_tail(
        client,
        &ap.room,
        ap.tail,
        ap.listen_self,
        whoami(ap)?,
        ap.output,
        ap.print_event_id,
    )
    .await // returning
}

/// Handle the --listen all CLI argument
pub(crate) async fn cli_listen_all(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Listen All chosen.");
    listen_all(
        client,
        &ap.room,
        ap.listen_self,
        whoami(ap)?,
        ap.output,
        ap.print_event_id,
    )
    .await // returning
}

/// Handle the --devices CLI argument
pub(crate) async fn cli_devices(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Devices chosen.");
    devices(client, ap.output, &ap.separator).await // returning
}

/// Utility function, returns user_id of itself
pub(crate) fn whoami(ap: &Args) -> Result<OwnedUserId, Error> {
    Ok(ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.user_id.clone())
}

/// Handle the --whoami CLI argument
pub(crate) fn cli_whoami(ap: &Args) -> Result<(), Error> {
    info!("Whoami chosen.");
    let whoami = whoami(ap)?;
    match ap.output {
        Output::Text => println!("{}", whoami),
        Output::JsonSpec => (),
        _ => println!("{{\"user_id\": \"{}\"}}", whoami),
    }
    Ok(())
}

/// Handle the --get-room-info CLI argument
pub(crate) async fn cli_get_room_info(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-room-info chosen.");
    // note that get_room_info vector is NOT empty.
    // If it were empty this function would not be called.
    get_room_info(client, &ap.get_room_info, ap.output, &ap.separator).await
}

/// Handle the --rooms CLI argument
pub(crate) async fn cli_rooms(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Rooms chosen.");
    rooms(client, ap.output).await
}

/// Handle the --invited-rooms CLI argument
pub(crate) async fn cli_invited_rooms(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Invited-rooms chosen.");
    invited_rooms(client, ap.output).await
}

/// Handle the --joined-rooms CLI argument
pub(crate) async fn cli_joined_rooms(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Joined-rooms chosen.");
    joined_rooms(client, ap.output).await
}

/// Handle the --left-rooms CLI argument
pub(crate) async fn cli_left_rooms(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Left-rooms chosen.");
    left_rooms(client, ap.output).await
}

/// Determine whether a new room should be encrypted based on visibility and --plain flag
fn should_encrypt(visibility: &Visibility, plain: Option<bool>) -> bool {
    match visibility {
        Visibility::Private => !plain.unwrap_or(false), // private rooms are encrypted by default
        Visibility::Public => !plain.unwrap_or(true),   // public rooms are plain by default
        _ => !plain.unwrap_or(false),
    }
}

/// Handle the --room-create CLI argument
pub(crate) async fn cli_room_create(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-create chosen.");
    room_create(
        client,
        false,
        should_encrypt(&ap.visibility, ap.plain),
        &[],
        &ap.room_create,
        &ap.name,
        &ap.topic,
        ap.output,
        ap.visibility.clone(),
        &ap.separator,
    )
    .await
}

/// Handle the --room-dm-create CLI argument
pub(crate) async fn cli_room_dm_create(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-dm-create chosen.");
    let mut users_to_create = ap.room_dm_create.clone();
    if !ap.room_dm_create_allow_duplicates {
        // Check for existing DM rooms with each user and skip duplicates
        let mut filtered_users = Vec::new();
        for user_str in &users_to_create {
            let user_id = match matrix_sdk::ruma::UserId::parse(user_str.as_str()) {
                Ok(uid) => uid,
                Err(_) => {
                    // Keep it; room_create will handle the error
                    filtered_users.push(user_str.clone());
                    continue;
                }
            };
            // Check all joined rooms for an existing DM with this user
            let mut found_dm = false;
            for room in client.joined_rooms() {
                if !room.is_direct().await.unwrap_or(false) {
                    continue;
                }
                let members = match room.members(matrix_sdk::RoomMemberships::ACTIVE).await {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if members.len() == 2
                    && members.iter().any(|m| m.user_id() == user_id)
                {
                    info!(
                        "DM room with user {:?} already exists: {}. Skipping creation.",
                        user_str,
                        room.room_id()
                    );
                    // Print the existing room id like Python does
                    match ap.output {
                        Output::Text => println!("{}", room.room_id()),
                        Output::JsonSpec => (),
                        _ => println!(
                            "{{\"room_id\": \"{}\"}}",
                            room.room_id()
                        ),
                    }
                    found_dm = true;
                    break;
                }
            }
            if !found_dm {
                filtered_users.push(user_str.clone());
            }
        }
        users_to_create = filtered_users;
    }
    if users_to_create.is_empty() {
        return Ok(());
    }
    room_create(
        client,
        true,
        should_encrypt(&ap.visibility, ap.plain),
        &users_to_create,
        &ap.alias,
        &ap.name,
        &ap.topic,
        ap.output,
        ap.visibility.clone(),
        &ap.separator,
    )
    .await
}

/// Handle the --room-leave CLI argument
pub(crate) async fn cli_room_leave(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-leave chosen.");
    room_leave(client, &ap.room_leave, ap.output).await
}

/// Handle the --room-forget CLI argument
pub(crate) async fn cli_room_forget(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-forget chosen.");
    room_forget(client, &ap.room_forget, ap.output).await
}

/// Handle the --room-invite CLI argument
pub(crate) async fn cli_room_invite(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-invite chosen.");
    room_invite(client, &ap.room_invite, &ap.user, ap.output).await
}

/// Handle the --room-join CLI argument
pub(crate) async fn cli_room_join(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-join chosen.");
    room_join(client, &ap.room_join, ap.output).await
}

/// Handle the --room-ban CLI argument
pub(crate) async fn cli_room_ban(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-ban chosen.");
    room_ban(client, &ap.room_ban, &ap.user, ap.output).await
}

/// Handle the --room-unban CLI argument
pub(crate) async fn cli_room_unban(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-unban chosen.");
    room_unban(client, &ap.room_unban, &ap.user, ap.output).await
}

/// Handle the --room-kick CLI argument
pub(crate) async fn cli_room_kick(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-kick chosen.");
    room_kick(client, &ap.room_kick, &ap.user, ap.output).await
}

/// Handle the --room-resolve_alias CLI argument
pub(crate) async fn cli_room_resolve_alias(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-resolve-alias chosen.");
    room_resolve_alias(client, &ap.room_resolve_alias, ap.output, &ap.separator).await
}

/// Handle the --room-enable-encryption CLI argument
pub(crate) async fn cli_room_enable_encryption(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-enable-encryption chosen.");
    room_enable_encryption(client, &ap.room_enable_encryption, ap.output).await
}

/// Handle the --get-avatar CLI argument
pub(crate) async fn cli_get_avatar(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-avatar chosen.");
    if let Some(path) = ap.get_avatar.as_ref() {
        get_avatar(client, path, ap.output).await
    } else {
        Err(Error::MissingCliParameter)
    }
}

/// Handle the --set-avatar CLI argument
pub(crate) async fn cli_set_avatar(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Set-avatar chosen.");
    if let Some(path) = ap.set_avatar.as_ref() {
        set_avatar(client, path, ap.output).await
    } else {
        Err(Error::MissingCliParameter)
    }
}

/// Handle the --get-avatar-url CLI argument
pub(crate) async fn cli_get_avatar_url(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-avatar-url chosen.");
    get_avatar_url(client, ap.output, &ap.separator).await
}

/// Handle the --set-avatar_url CLI argument
pub(crate) async fn cli_set_avatar_url(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Set-avatar-url chosen.");
    if let Some(mxc_uri) = ap.set_avatar_url.as_ref() {
        set_avatar_url(client, mxc_uri, ap.output).await
    } else {
        Err(Error::MissingCliParameter)
    }
}

/// Handle the --unset-avatar_url CLI argument
pub(crate) async fn cli_unset_avatar_url(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Unset-avatar-url chosen.");
    unset_avatar_url(client, ap.output).await
}

/// Handle the --get-display-name CLI argument
pub(crate) async fn cli_get_display_name(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-display-name chosen.");
    get_display_name(client, ap.output, &ap.separator).await
}

/// Handle the --set-display-name CLI argument
pub(crate) async fn cli_set_display_name(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Set-display-name chosen.");
    if let Some(name) = ap.set_display_name.as_ref() {
        set_display_name(client, name, ap.output).await
    } else {
        Err(Error::MissingCliParameter)
    }
}

/// Handle the --get-profile CLI argument
pub(crate) async fn cli_get_profile(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-profile chosen.");
    get_profile(client, ap.output, &ap.separator).await
}

/// Handle the --get-masterkey CLI argument
pub(crate) async fn cli_get_masterkey(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-masterkey chosen.");
    get_masterkey(
        client,
        ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.user_id.clone(),
        ap.output,
    )
    .await
}

/// Handle the --room-get-visibility CLI argument
pub(crate) async fn cli_room_get_visibility(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-get-visibility chosen.");
    room_get_visibility(client, &ap.room_get_visibility, ap.output, &ap.separator).await
}

/// Handle the --room-get-state CLI argument
pub(crate) async fn cli_room_get_state(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-get-state chosen.");
    room_get_state(client, &ap.room_get_state, ap.output, &ap.separator).await
}

/// Handle the --joined-members CLI argument
pub(crate) async fn cli_joined_members(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Joined-members chosen.");
    joined_members(client, &ap.joined_members, ap.output, &ap.separator).await
}

/// Handle the --delete-device CLI argument
pub(crate) async fn cli_delete_device(client: &Client, ap: &mut Args) -> Result<(), Error> {
    info!("Delete-device chosen.");
    delete_devices_pre(client, ap).await
}

/// Handle the --logout CLI argument
pub(crate) async fn cli_logout(client: &Client, ap: &mut Args) -> Result<(), Error> {
    info!("Logout chosen.");
    if ap.logout.is_none() {
        return Ok(());
    }
    if ap.logout.is_all() {
        // delete_device list will be overwritten, but that is okay because
        // logout is the last function in main.
        ap.delete_device = vec!["*".to_owned()];
        match cli_delete_device(client, ap).await {
            Ok(_) => info!("Logout caused all devices to be deleted."),
            Err(e) => error!(
                "Error: Failed to delete all devices, but we remove local device id anyway. {:?}",
                e
            ),
        }
    }
    logout(client, ap).await
}

/// Handle the --set-device-name CLI argument
pub(crate) async fn cli_set_device_name(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Set-device-name chosen.");
    if let Some(name) = ap.set_device_name.as_ref() {
        set_device_name(client, name, ap.output).await
    } else {
        Err(Error::MissingCliParameter)
    }
}

/// Handle the --set-presence CLI argument
pub(crate) async fn cli_set_presence(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Set-presence chosen.");
    if let Some(presence) = ap.set_presence.as_ref() {
        set_presence(client, presence, ap.output).await
    } else {
        Err(Error::MissingCliParameter)
    }
}

/// Handle the --get-presence CLI argument
pub(crate) async fn cli_get_presence(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-presence chosen.");
    get_presence(client, ap.output, &ap.separator).await
}

/// Handle the --room-set-alias CLI argument
pub(crate) async fn cli_room_set_alias(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-set-alias chosen.");
    room_set_alias(client, &ap.room, &ap.room_set_alias, ap.output).await
}

/// Handle the --room-delete-alias CLI argument
pub(crate) async fn cli_room_delete_alias(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-delete-alias chosen.");
    room_delete_alias(client, &ap.room_delete_alias, ap.output).await
}

/// Handle the --discovery-info CLI argument
pub(crate) async fn cli_discovery_info(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Discovery-info chosen.");
    discovery_info(client, ap.output, &ap.separator).await
}

/// Handle the --login-info CLI argument
pub(crate) async fn cli_login_info(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Login-info chosen.");
    login_info(client, ap.output).await
}

/// Handle the --content-repository-config CLI argument
pub(crate) async fn cli_content_repository_config(
    client: &Client,
    ap: &Args,
) -> Result<(), Error> {
    info!("Content-repository-config chosen.");
    content_repository_config(client, ap.output).await
}

/// Handle the --delete-mxc-before CLI argument
pub(crate) async fn cli_delete_mxc_before(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Delete-mxc-before chosen.");
    let creds = ap.creds.as_ref().ok_or(Error::NoCredentialsFound)?;
    delete_mxc_before(client, &ap.delete_mxc_before, creds, ap.output).await
}

/// Handle the --get-client-info CLI argument
pub(crate) async fn cli_get_client_info(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Get-client-info chosen.");
    get_client_info(client, ap).await
}

/// Handle the --room-invites CLI argument
pub(crate) async fn cli_room_invites(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-invites chosen.");
    if let Some(ref mode) = ap.room_invites {
        let mode_lower = mode.to_lowercase();
        if mode_lower != "list" && mode_lower != "join" && mode_lower != "list+join" {
            error!(
                "For --room-invites currently only \"list\", \"join\" or \"list+join\" are allowed as keywords. Got: {:?}",
                mode
            );
            return Err(Error::UnsupportedCliParameter(
                "Invalid --room-invites argument. Use list, join, or list+join.",
            ));
        }
        crate::mclient::room_invites(client, &mode_lower, ap.output, &ap.separator).await
    } else {
        Ok(())
    }
}

/// Handle the --room-redact CLI argument
pub(crate) async fn cli_room_redact(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Room-redact chosen.");
    if ap.room_redact.is_empty() {
        return Ok(());
    }
    room_redact(client, &ap.room_redact, ap.output).await
}

/// Handle the --has-permission CLI argument
pub(crate) async fn cli_has_permission(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Has-permission chosen.");
    let user_id = &ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.user_id;
    has_permission(client, &ap.has_permission, user_id, ap.output, &ap.separator).await
}

/// Handle the --joined-dm-rooms CLI argument
pub(crate) async fn cli_joined_dm_rooms(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("Joined-dm-rooms chosen.");
    let user_id = &ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.user_id;
    joined_dm_rooms(client, &ap.joined_dm_rooms, user_id, ap.output, &ap.separator).await
}

/// Handle the --rest CLI argument
pub(crate) async fn cli_rest(client: &Client, ap: &Args) -> Result<(), Error> {
    info!("REST chosen.");
    let credentials = ap.creds.as_ref().ok_or(Error::NotLoggedIn)?;
    rest(
        client,
        &ap.rest,
        credentials,
        ap.access_token.as_deref(),
        ap.output,
    )
    .await
}
