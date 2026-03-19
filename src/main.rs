//! # matrix-commander-ng
//!
//! A maintained, actively-developed CLI client for Matrix, written in Rust.
//! Fork of [matrix-commander-rs](https://github.com/8go/matrix-commander-rs)
//! with output equalized to match the Python
//! [matrix-commander](https://github.com/8go/matrix-commander).
//!
//! matrix-commander-ng is a terminal-based CLI client of
//! Matrix <https://matrix.org>. It lets you login to your Matrix account,
//! verify your devices, and send encrypted (or unencrypted) messages
//! and files on the Matrix network.
//!
//! ## Usage
//!
//! ```sh
//! matrix-commander-ng --login password          # first time login
//! matrix-commander-ng --verify emoji             # emoji verification
//! matrix-commander-ng --message "Hello World"    # send a message
//! matrix-commander-ng --file photo.jpg           # send a file
//! matrix-commander-ng --listen once              # receive messages
//! matrix-commander-ng --login sso                # SSO login
//! ```
//!
//! See <https://github.com/longregen/matrix-commander-ng> for full docs.

use clap::{CommandFactory, Parser};
use colored::Colorize;
use std::cmp::Ordering;
use std::env;
use std::io::{self, stdin, stdout, IsTerminal};
use std::path::PathBuf;
use std::sync::LazyLock;
use tracing::{debug, enabled, error, info, warn, Level};
use tracing_subscriber::EnvFilter;

// matrix_sdk::Client is used via crate::mclient

pub mod types;
pub use types::*;

pub mod args;
pub use args::Args;

/// import matrix-sdk Client related code of general kind: login, logout, verify, sync, etc
mod mclient;
use crate::mclient::{
    convert_to_full_alias_ids, convert_to_full_mxc_uris, convert_to_full_room_ids,
    convert_to_full_user_ids, logout_local, replace_star_with_rooms,
};

// import matrix-sdk Client related code related to receiving messages and listening
mod listen;

/// CLI handler functions and helpers
pub mod cli;
use crate::cli::*;

/// the version number from Cargo.toml at compile time
const VERSION_O: Option<&str> = option_env!("CARGO_PKG_VERSION");
/// fallback if static compile time value is None
const VERSION: &str = "unknown version";
/// the package name from Cargo.toml at compile time, usually matrix-commander
const PKG_NAME_O: Option<&str> = option_env!("CARGO_PKG_NAME");
/// fallback if static compile time value is None
const PKG_NAME: &str = "matrix-commander-ng";
/// the name of binary program from Cargo.toml at compile time
const BIN_NAME_O: Option<&str> = option_env!("CARGO_BIN_NAME");
/// fallback if static compile time value is None
const BIN_NAME: &str = "matrix-commander-ng";
/// fallback if static compile time value is None
const BIN_NAME_UNDERSCORE: &str = "matrix_commander_ng";
/// the repo name from Cargo.toml at compile time
const PKG_REPOSITORY_O: Option<&str> = option_env!("CARGO_PKG_REPOSITORY");
/// fallback if static compile time value is None
const PKG_REPOSITORY: &str = "https://github.com/longregen/matrix-commander-ng";
/// default name for login credentials JSON file
const CREDENTIALS_FILE_DEFAULT: &str = "credentials.json";
/// depreciated default directory to be used for persistent storage
const DEPRECIATED_STORE_DIR_DEFAULT: &str = "sledstore/";
/// default directory to be used by end-to-end encrypted protocol for persistent storage
const STORE_DIR_DEFAULT: &str = "store/";
/// default timeouts for waiting for the Matrix server, in seconds
pub const TIMEOUT_DEFAULT: u64 = 60;
/// URL for README.md file downloaded for --readme
const URL_README: &str =
    "https://raw.githubusercontent.com/longregen/matrix-commander-ng/main/README.md";

/// Gets version number, static if available, otherwise default.
pub fn get_version() -> &'static str {
    VERSION_O.unwrap_or(VERSION)
}

/// Gets Rust package name, static if available, otherwise default.
pub fn get_pkg_name() -> &'static str {
    PKG_NAME_O.unwrap_or(PKG_NAME)
}

/// Gets Rust binary name, static if available, otherwise default.
fn get_bin_name() -> &'static str {
    BIN_NAME_O.unwrap_or(BIN_NAME)
}

/// Gets Rust package repository, static if available, otherwise default.
pub fn get_pkg_repository() -> &'static str {
    PKG_REPOSITORY_O.unwrap_or(PKG_REPOSITORY)
}

/// Gets program name without extension.
pub fn get_prog_without_ext() -> &'static str {
    get_bin_name()
}

/// Gets the *default* path (including file name) of the credentials file
pub fn get_credentials_default_path() -> PathBuf {
    let dir =
        directories::ProjectDirs::from_path(PathBuf::from(get_prog_without_ext())).unwrap();
    let dp = dir.data_dir().join(CREDENTIALS_FILE_DEFAULT);
    debug!(
        "Data will be put into project directory {:?}.",
        dir.data_dir()
    );
    info!("Credentials file with access token is {}.", dp.display());
    dp
}

/// Gets the *default* path (terminating in a directory) of the store directory
pub fn get_store_default_path() -> PathBuf {
    let dir =
        directories::ProjectDirs::from_path(PathBuf::from(get_prog_without_ext())).unwrap();
    let dp = dir.data_dir().join(STORE_DIR_DEFAULT);
    debug!("Default project directory is {:?}.", dir.data_dir());
    info!("Default store directory is {}.", dp.display());
    dp
}

/// Gets the depreciated default path of the store directory (for migration)
pub fn get_store_depreciated_default_path() -> PathBuf {
    let dir =
        directories::ProjectDirs::from_path(PathBuf::from(get_prog_without_ext())).unwrap();
    let dp = dir.data_dir().join(DEPRECIATED_STORE_DIR_DEFAULT);
    dp
}

/// Prints the usage info
pub fn usage() {
    let help_str = Args::command().render_usage().to_string();
    println!("{}", &help_str);
    println!("Options:");
    let help_str = Args::command().render_help().to_string();
    let v: Vec<&str> = help_str.split('\n').collect();
    for l in v {
        if l.starts_with("  -") || l.starts_with("      --") {
            println!("{}", &l);
        }
    }
}

/// Prints the short help
pub fn help() {
    static HELP_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?P<del>[ ]+Details::[\S\s]*?)(?P<keep>\nPS:|\n  -|\n      --)").unwrap()
    });
    let help_str = Args::command().render_help().to_string();
    let after = HELP_RE.replace_all(&help_str, "$keep");
    print!("{}", &after.replace("\n\n", "\n"));
    println!("Use --manual to get more detailed help information.");
}

/// Prints the long help
pub fn manual() {
    let help_str = Args::command().render_long_help().to_string();
    println!("{}", &help_str);
}

/// Prints the README.md file
pub async fn readme() {
    match reqwest::get(URL_README).await {
        Ok(resp) => {
            debug!("Got README.md file from URL {:?}.", URL_README);
            println!("{}", resp.text().await.unwrap())
        }
        Err(ref e) => {
            println!(
                "Error getting README.md from {:#?}. Reported error {:?}.",
                URL_README, e
            );
        }
    };
}

/// Prints the version information
pub fn version(output: Output) {
    let version = if stdout().is_terminal() {
        get_version().green()
    } else {
        get_version().normal()
    };
    match output {
        Output::Text => {
            println!();
            println!(
                "  _|      _|      _|_|_|                     {}",
                get_prog_without_ext()
            );
            print!("  _|_|  _|_|    _|             _~^~^~_       ");
            println!("a rusty vision of a Matrix CLI client");
            println!(
                "  _|  _|  _|    _|         \\) /  o o  \\ (/   version {}",
                version
            );
            println!(
                "  _|      _|    _|           '_   -   _'     repo {}",
                get_pkg_repository()
            );
            print!("  _|      _|      _|_|_|     / '-----' \\     ");
            println!("please submit PRs to make the vision a reality");
            println!();
        }
        Output::JsonSpec => (),
        _ => println!(
            "{{\"program\": {:?}, \"version\": {:?}, \"repo\": {:?}}}",
            get_prog_without_ext(),
            get_version(),
            get_pkg_repository()
        ),
    }
}

/// Prints the installed version and the latest crates.io-available version
pub async fn version_check() {
    println!("Installed version: v{}", get_version());
    let name = env!("CARGO_PKG_NAME");
    let current = env!("CARGO_PKG_VERSION");
    let url = format!("https://crates.io/api/v1/crates/{}", name);
    let result: Result<Option<String>, String> = async {
        let resp = reqwest::Client::new()
            .get(&url)
            .header("User-Agent", format!("{}/{}", name, current))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let latest = json["crate"]["max_version"]
            .as_str()
            .map(|s| s.to_string());
        Ok(latest)
    }
    .await;
    let avail = "New version is available";
    let uptod = "You are up-to-date.";
    let couldnot = "Could not get latest version.";
    let (available, uptodate, couldnotget) = if stdout().is_terminal() {
        (avail.yellow(), uptod.green(), couldnot.red())
    } else {
        (avail.normal(), uptod.normal(), couldnot.normal())
    };
    match result {
        Ok(Some(ref latest)) if latest != current => {
            println!(
                "{} on https://crates.io/crates/{}: {}",
                available, name, latest
            );
        }
        Ok(_) => println!("{uptodate} You already have the latest version."),
        Err(ref e) => println!("{couldnotget} Error reported: {e}."),
    };
}

/// Asks the public for help
pub fn contribute() {
    println!();
    println!(
        "{} is a maintained fork of matrix-commander-rs by 8go.",
        get_prog_without_ext()
    );
    println!("Contributions are welcome! Have a look at the repo ");
    println!("{}.", get_pkg_repository());
    println!("Please open issues, submit pull requests, and help make ");
    println!("{} better for everyone.", get_prog_without_ext());
}

/// We need your code contributions! Please add features and make PRs! :pray: :clap:
#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut ap = Args::parse();
    let mut errcount = 0;
    let mut result: Result<(), Error> = Ok(());

    // handle log level and debug options
    let env_org_rust_log = env::var("RUST_LOG").unwrap_or_default().to_uppercase();
    match ap.debug.cmp(&1) {
        Ordering::Equal => {
            let llvec = vec![LogLevel::Debug];
            ap.log_level = Some(llvec);
        }
        Ordering::Greater => {
            ap.log_level = Some(vec![LogLevel::Debug, LogLevel::Debug]);
        }
        Ordering::Less => (),
    }
    match ap.log_level.as_ref() {
        None => {
            let filter = if std::env::var("RUST_LOG").is_ok() {
                EnvFilter::from_default_env()
            } else {
                EnvFilter::new(format!("error,{}=info", BIN_NAME_UNDERSCORE))
            };
            tracing_subscriber::fmt()
                .with_writer(io::stderr)
                .with_env_filter(filter)
                .init();
            debug!("Neither --debug nor --log-level was used. Using environment variable RUST_LOG or default info.");
        }
        Some(llvec) => {
            if llvec.len() == 1 {
                if llvec[0].is_none() {
                    return Err(Error::UnsupportedCliParameter(
                        "Value 'none' not allowed for --log-level argument",
                    ));
                }
                let mut rlogstr: String = BIN_NAME_UNDERSCORE.to_owned();
                rlogstr.push('=');
                rlogstr.push_str(&llvec[0].to_string());
                tracing_subscriber::fmt()
                    .with_writer(io::stderr)
                    .with_env_filter(rlogstr.clone())
                    .init();
                debug!(
                    "The --debug or --log-level was used once or with one value. \
                    Specifying logging equivalent to RUST_LOG seting of '{}'.",
                    rlogstr
                );
            } else {
                if llvec[0].is_none() || llvec[1].is_none() {
                    return Err(Error::UnsupportedCliParameter(
                        "Value 'none' not allowed for --log-level argument",
                    ));
                }
                let mut rlogstr: String = llvec[1].to_string().to_owned();
                rlogstr.push(',');
                rlogstr.push_str(BIN_NAME_UNDERSCORE);
                rlogstr.push('=');
                rlogstr.push_str(&llvec[0].to_string());
                tracing_subscriber::fmt()
                    .with_writer(io::stderr)
                    .with_env_filter(rlogstr.clone())
                    .init();
                debug!(
                    "The --debug or --log-level was used twice or with two values. \
                    Specifying logging equivalent to RUST_LOG seting of '{}'.",
                    rlogstr
                );
            }
            if llvec.len() > 2 {
                debug!("The --log-level option was incorrectly used more than twice. Ignoring third and further use.")
            }
        }
    }
    if ap.debug > 0 {
        info!("The --debug option overwrote the --log-level option.")
    }
    if ap.debug > 2 {
        debug!("The --debug option was incorrectly used more than twice. Ignoring third and further use.")
    }
    debug!("Original RUST_LOG env var is '{}'", env_org_rust_log);
    debug!("Final log-level option is {:?}", ap.log_level);
    if enabled!(Level::TRACE) {
        debug!(
            "Log level of module {} is set to TRACE.",
            get_prog_without_ext()
        );
    } else if enabled!(Level::DEBUG) {
        debug!(
            "Log level of module {} is set to DEBUG.",
            get_prog_without_ext()
        );
    }

    // Validate SSL options
    if ap.no_ssl && ap.ssl_certificate.is_some() {
        error!("Cannot use both --no-ssl and --ssl-certificate at the same time.");
        return Err(Error::UnsupportedCliParameter(
            "Cannot use both --no-ssl and --ssl-certificate at the same time.",
        ));
    }

    // Validate proxy option
    if let Some(ref proxy) = ap.proxy {
        if proxy.is_empty() {
            ap.proxy = None;
        } else if !(proxy.starts_with("http://")
            || proxy.starts_with("socks4://")
            || proxy.starts_with("socks5://"))
        {
            error!(
                "Proxy URL must start with 'http://', 'socks4://', or 'socks5://'. \
                Your proxy is set to {:?}.",
                proxy
            );
            return Err(Error::UnsupportedCliParameter(
                "Invalid proxy URL. Must start with http://, socks4://, or socks5://.",
            ));
        }
    }

    // Process separator escape sequences
    ap.separator = ap
        .separator
        .replace("\\t", "\t")
        .replace("\\n", "\n")
        .replace("\\\\", "\\");

    match ap.version {
        None => (),
        Some(None) => crate::version(ap.output),
        Some(Some(Version::Check)) => crate::version_check().await,
    }
    if ap.contribute {
        crate::contribute();
    };
    if ap.usage {
        crate::usage();
        return Ok(());
    };
    if ap.help {
        crate::help();
        return Ok(());
    };
    if ap.manual {
        crate::manual();
        return Ok(());
    };
    if ap.readme {
        crate::readme().await;
        return Ok(());
    };

    // -m not used but data being piped into stdin?
    if ap.message.is_empty() && !stdin().is_terminal() {
        debug!(
            "-m is empty, but there is something piped into stdin. Let's assume '-m -' \
            and read and send the information piped in on stdin."
        );
        ap.message.push("-".to_string());
    };

    if !(!ap.login.is_none()
        // get actions
        || ap.whoami
        || ap.bootstrap
        || !ap.verify.is_none()
        || ap.devices
        || !ap.get_room_info.is_empty()
        || ap.rooms
        || ap.invited_rooms
        || ap.joined_rooms
        || ap.left_rooms
        || !ap.room_get_visibility.is_empty()
        || !ap.room_get_state.is_empty()
        || !ap.joined_members.is_empty()
        || !ap.room_resolve_alias.is_empty()
        || ap.get_avatar.is_some()
        || ap.get_avatar_url
        || ap.get_display_name
        || ap.get_profile
        || !ap.media_download.is_empty()
        || !ap.media_mxc_to_http.is_empty()
        || ap.get_masterkey
        || ap.get_openid_token
        || ap.get_presence
        || ap.discovery_info
        || ap.login_info
        || ap.content_repository_config
        || ap.get_client_info
        || ap.room_invites.is_some()
        || !ap.has_permission.is_empty()
        || !ap.joined_dm_rooms.is_empty()
        || !ap.rest.is_empty()
        // set actions
        || !ap.room_create.is_empty()
        || !ap.room_dm_create.is_empty()
        || !ap.room_leave.is_empty()
        || !ap.room_forget.is_empty()
        || !ap.room_invite.is_empty()
        || !ap.room_join.is_empty()
        || !ap.room_ban.is_empty()
        || !ap.room_unban.is_empty()
        || !ap.room_kick.is_empty()
        || !ap.delete_device.is_empty()
        || ap.set_avatar.is_some()
        || ap.set_avatar_url.is_some()
        || ap.unset_avatar_url
        || ap.set_display_name.is_some()
        || !ap.room_enable_encryption.is_empty()
        || !ap.media_upload.is_empty()
        || !ap.media_delete.is_empty()
        || !ap.import_keys.is_empty()
        || !ap.export_keys.is_empty()
        || ap.set_device_name.is_some()
        || ap.set_presence.is_some()
        || !ap.room_set_alias.is_empty()
        || !ap.room_delete_alias.is_empty()
        || !ap.delete_mxc_before.is_empty()
        || !ap.room_redact.is_empty()
        // send and listen actions
        || !ap.message.is_empty()
        || !ap.file.is_empty()
        || !ap.image.is_empty()
        || !ap.audio.is_empty()
        || !ap.event.is_empty()
        || ap.listen.is_once()
        || ap.listen.is_forever()
        || ap.listen.is_tail()
        || ap.tail > 0
        || ap.listen.is_all()
        || !ap.logout.is_none())
    {
        debug!("There are no more actions to take. No need to connect to server. Quitting.");
        debug!("Good bye");
        return Ok(());
    }
    let (clientres, credentials) = if !ap.login.is_none() {
        match cli_login(&mut ap).await {
            Ok((client, credentials)) => (Ok(client), credentials),
            Err(Error::LoginUnnecessary) => {
                return Err(Error::LoginUnnecessary);
            }
            Err(ref e) => {
                error!(
                    "Login to server failed or credentials information could not be \
                    written to disk. Check your arguments and try --login again. \
                    Reported error is: {:?}",
                    e
                );
                return Err(Error::LoginFailed);
            }
        }
    } else if let Ok(credentials) = restore_credentials(&ap) {
        let needs_sync = !ap.room_leave.is_empty()
            || !ap.room_forget.is_empty()
            || !ap.room_invite.is_empty()
            || !ap.room_join.is_empty()
            || !ap.room_ban.is_empty()
            || !ap.room_unban.is_empty()
            || !ap.room_kick.is_empty()
            || !ap.room_enable_encryption.is_empty()
            || !ap.room_set_alias.is_empty()
            || !ap.room_delete_alias.is_empty()
            || !ap.room_resolve_alias.is_empty()
            || !ap.room_redact.is_empty()
            || !ap.room_create.is_empty()
            || !ap.room_dm_create.is_empty()
            || !ap.get_room_info.is_empty()
            || ap.rooms
            || ap.invited_rooms
            || ap.joined_rooms
            || ap.left_rooms
            || !ap.room_get_visibility.is_empty()
            || !ap.room_get_state.is_empty()
            || !ap.joined_members.is_empty()
            || !ap.has_permission.is_empty()
            || !ap.joined_dm_rooms.is_empty()
            || ap.room_invites.is_some()
            || !ap.message.is_empty()
            || !ap.file.is_empty()
            || !ap.image.is_empty()
            || !ap.audio.is_empty()
            || !ap.event.is_empty()
            || ap.listen.is_once()
            || ap.listen.is_forever()
            || ap.listen.is_tail()
            || ap.tail > 0
            || ap.listen.is_all()
            || ap.bootstrap
            || !ap.verify.is_none()
            || !ap.logout.is_none();
        (cli_restore_login(&credentials, &ap, needs_sync).await, credentials)
    } else {
        error!(
            "Credentials file does not exists or cannot be read. \
            Consider doing a '--logout' to clean up, then perform a '--login'."
        );
        return Err(Error::LoginFailed);
    };
    ap.creds = Some(credentials);

    // Place all the calls here that work without a server connection
    if ap.whoami {
        match cli_whoami(&ap) {
            Ok(ref _n) => debug!("cli_whoami successful"),
            Err(e) => {
                error!("Error: cli_whoami reported {}", e);
                errcount += 1;
                result = Err(e);
            }
        };
    };

    convert_to_full_mxc_uris(
        &mut ap.media_mxc_to_http,
        ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.homeserver.host_str().unwrap(),
    )
    .await;

    if !ap.media_mxc_to_http.is_empty() {
        match cli_media_mxc_to_http(&ap).await {
            Ok(ref _n) => debug!("cli_media_mxc_to_http successful"),
            Err(e) => {
                error!("Error: cli_media_mxc_to_http reported {}", e);
                errcount += 1;
                result = Err(e);
            }
        };
    };

    match clientres {
        Ok(client) => {
            debug!("A valid client connection has been established with server.");
            let default_room =
                get_room_default_from_credentials(&client, ap.creds.as_ref().ok_or(Error::NotLoggedIn)?).await;
            let hostname = ap.creds.as_ref().ok_or(Error::NotLoggedIn)?.homeserver.host_str().unwrap().to_owned();
            let hostname = hostname.as_str();
            set_rooms(&mut ap, &default_room);
            set_users(&mut ap)?;

            replace_minus_with_default_room(&mut ap.room_leave, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_leave, hostname).await;

            replace_minus_with_default_room(&mut ap.room_forget, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_forget, hostname).await;

            convert_to_full_room_aliases(&mut ap.room_resolve_alias, hostname);
            convert_to_full_room_aliases(&mut ap.room_set_alias, hostname);
            convert_to_full_room_aliases(&mut ap.room_delete_alias, hostname);

            replace_minus_with_default_room(&mut ap.room_enable_encryption, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_enable_encryption, hostname).await;

            replace_minus_with_default_room(&mut ap.get_room_info, &default_room);
            convert_to_full_room_ids(&client, &mut ap.get_room_info, hostname).await;

            replace_minus_with_default_room(&mut ap.room_invite, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_invite, hostname).await;

            replace_minus_with_default_room(&mut ap.room_join, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_join, hostname).await;

            replace_minus_with_default_room(&mut ap.room_ban, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_ban, hostname).await;

            replace_minus_with_default_room(&mut ap.room_unban, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_unban, hostname).await;

            replace_minus_with_default_room(&mut ap.room_kick, &default_room);
            convert_to_full_room_ids(&client, &mut ap.room_kick, hostname).await;

            replace_minus_with_default_room(&mut ap.room_get_visibility, &default_room);
            replace_star_with_rooms(&client, &mut ap.room_get_visibility);
            convert_to_full_room_ids(&client, &mut ap.room_get_visibility, hostname).await;

            replace_minus_with_default_room(&mut ap.room_get_state, &default_room);
            replace_star_with_rooms(&client, &mut ap.room_get_state);
            convert_to_full_room_ids(&client, &mut ap.room_get_state, hostname).await;

            replace_minus_with_default_room(&mut ap.joined_members, &default_room);
            replace_star_with_rooms(&client, &mut ap.joined_members);
            convert_to_full_room_ids(&client, &mut ap.joined_members, hostname).await;

            convert_to_full_user_ids(&mut ap.room_dm_create, hostname);
            ap.room_dm_create.retain(|x| !x.trim().is_empty());

            convert_to_full_alias_ids(&mut ap.alias, hostname);
            ap.alias.retain(|x| !x.trim().is_empty());

            convert_to_full_mxc_uris(&mut ap.media_download, hostname).await;
            convert_to_full_mxc_uris(&mut ap.media_delete, hostname).await;

            if ap.tail > 0 {
                if !ap.listen.is_never() && !ap.listen.is_tail() {
                    warn!(
                        "Two contradicting listening methods were specified. \
                    Overwritten with --tail. Will use '--listen tail'. {:?} {}",
                        ap.listen, ap.tail
                    )
                }
                ap.listen = Listen::Tail
            }

            // top-priority actions

            if ap.bootstrap {
                match cli_bootstrap(&client, &mut ap).await {
                    Ok(ref _n) => debug!("cli_bootstrap successful"),
                    Err(e) => {
                        error!("Error: cli_bootstrap reported {}", e);
                        errcount += 1;
                        result = Err(e);
                    }
                };
            };

            if !ap.verify.is_none() {
                match cli_verify(&client, &ap).await {
                    Ok(ref _n) => debug!("cli_verify successful"),
                    Err(e) => {
                        error!("Error: cli_verify reported {}", e);
                        errcount += 1;
                        result = Err(e);
                    }
                };
            };

            // get actions

            macro_rules! dispatch {
                ($cond:expr, $call:expr, $name:expr) => {
                    if $cond {
                        match $call {
                            Ok(ref _n) => debug!("{} successful", $name),
                            Err(e) => {
                                error!("Error: {} reported {}", $name, e);
                                errcount += 1;
                                result = Err(e);
                            }
                        };
                    }
                };
            }

            dispatch!(ap.devices, cli_devices(&client, &ap).await, "devices");
            dispatch!(!ap.get_room_info.is_empty(), cli_get_room_info(&client, &ap).await, "get_room_info");
            dispatch!(ap.rooms, cli_rooms(&client, &ap).await, "rooms");
            dispatch!(ap.invited_rooms, cli_invited_rooms(&client, &ap).await, "invited_rooms");
            dispatch!(ap.joined_rooms, cli_joined_rooms(&client, &ap).await, "joined_rooms");
            dispatch!(ap.left_rooms, cli_left_rooms(&client, &ap).await, "left_rooms");
            dispatch!(!ap.room_get_visibility.is_empty(), cli_room_get_visibility(&client, &ap).await, "room_get_visibility");
            dispatch!(!ap.room_get_state.is_empty(), cli_room_get_state(&client, &ap).await, "room_get_state");
            dispatch!(!ap.joined_members.is_empty(), cli_joined_members(&client, &ap).await, "joined_members");
            dispatch!(!ap.room_resolve_alias.is_empty(), cli_room_resolve_alias(&client, &ap).await, "room_resolve_alias");
            dispatch!(ap.get_avatar.is_some(), cli_get_avatar(&client, &ap).await, "get_avatar");
            dispatch!(ap.get_avatar_url, cli_get_avatar_url(&client, &ap).await, "get_avatar_url");
            dispatch!(ap.get_display_name, cli_get_display_name(&client, &ap).await, "get_display_name");
            dispatch!(ap.get_profile, cli_get_profile(&client, &ap).await, "get_profile");
            dispatch!(ap.get_masterkey, cli_get_masterkey(&client, &ap).await, "get_masterkey");
            dispatch!(!ap.media_download.is_empty(), cli_media_download(&client, &ap).await, "media_download");
            dispatch!(ap.get_openid_token, cli_get_openid_token(&client, &ap).await, "get_openid_token");
            dispatch!(ap.get_presence, cli_get_presence(&client, &ap).await, "get_presence");
            dispatch!(ap.discovery_info, cli_discovery_info(&client, &ap).await, "discovery_info");
            dispatch!(ap.login_info, cli_login_info(&client, &ap).await, "login_info");
            dispatch!(ap.content_repository_config, cli_content_repository_config(&client, &ap).await, "content_repository_config");
            dispatch!(ap.get_client_info, cli_get_client_info(&client, &ap).await, "get_client_info");
            dispatch!(ap.room_invites.is_some(), cli_room_invites(&client, &ap).await, "room_invites");
            dispatch!(!ap.has_permission.is_empty(), cli_has_permission(&client, &ap).await, "has_permission");
            dispatch!(!ap.joined_dm_rooms.is_empty(), cli_joined_dm_rooms(&client, &ap).await, "joined_dm_rooms");
            dispatch!(!ap.rest.is_empty(), cli_rest(&client, &ap).await, "rest");

            // set actions
            dispatch!(!ap.room_create.is_empty(), cli_room_create(&client, &ap).await, "room_create");
            dispatch!(!ap.room_dm_create.is_empty(), cli_room_dm_create(&client, &ap).await, "room_dm_create");

            dispatch!(!ap.room_leave.is_empty(), cli_room_leave(&client, &ap).await, "room_leave");
            dispatch!(!ap.room_forget.is_empty(), cli_room_forget(&client, &ap).await, "room_forget");

            dispatch!(!ap.room_invite.is_empty(), cli_room_invite(&client, &ap).await, "room_invite");
            dispatch!(!ap.room_join.is_empty(), cli_room_join(&client, &ap).await, "room_join");
            dispatch!(!ap.room_ban.is_empty(), cli_room_ban(&client, &ap).await, "room_ban");
            dispatch!(!ap.room_unban.is_empty(), cli_room_unban(&client, &ap).await, "room_unban");
            dispatch!(!ap.room_kick.is_empty(), cli_room_kick(&client, &ap).await, "room_kick");
            dispatch!(!ap.delete_device.is_empty(), cli_delete_device(&client, &mut ap).await, "delete_device");
            dispatch!(ap.set_avatar.is_some(), cli_set_avatar(&client, &ap).await, "set_avatar");
            dispatch!(ap.set_avatar_url.is_some(), cli_set_avatar_url(&client, &ap).await, "set_avatar_url");
            dispatch!(ap.unset_avatar_url, cli_unset_avatar_url(&client, &ap).await, "unset_avatar_url");
            dispatch!(ap.set_display_name.is_some(), cli_set_display_name(&client, &ap).await, "set_display_name");
            dispatch!(!ap.room_enable_encryption.is_empty(), cli_room_enable_encryption(&client, &ap).await, "room_enable_encryption");
            dispatch!(!ap.media_upload.is_empty(), cli_media_upload(&client, &ap).await, "media_upload");
            dispatch!(!ap.media_delete.is_empty(), cli_media_delete(&client, &ap).await, "media_delete");
            dispatch!(!ap.import_keys.is_empty(), cli_import_keys(&client, &ap).await, "import_keys");
            dispatch!(!ap.export_keys.is_empty(), cli_export_keys(&client, &ap).await, "export_keys");
            dispatch!(ap.set_device_name.is_some(), cli_set_device_name(&client, &ap).await, "set_device_name");
            dispatch!(ap.set_presence.is_some(), cli_set_presence(&client, &ap).await, "set_presence");
            dispatch!(!ap.room_set_alias.is_empty(), cli_room_set_alias(&client, &ap).await, "room_set_alias");
            dispatch!(!ap.room_delete_alias.is_empty(), cli_room_delete_alias(&client, &ap).await, "room_delete_alias");
            dispatch!(!ap.delete_mxc_before.is_empty(), cli_delete_mxc_before(&client, &ap).await, "delete_mxc_before");
            dispatch!(!ap.room_redact.is_empty(), cli_room_redact(&client, &ap).await, "room_redact");

            // send actions
            dispatch!(!ap.message.is_empty(), cli_message(&client, &ap).await, "message");
            dispatch!(!ap.file.is_empty(), cli_file(&client, &ap).await, "file");
            dispatch!(!ap.image.is_empty(), cli_image(&client, &ap).await, "image");
            dispatch!(!ap.audio.is_empty(), cli_audio(&client, &ap).await, "audio");
            dispatch!(!ap.event.is_empty(), cli_event(&client, &ap).await, "event");

            // listen actions
            dispatch!(ap.listen.is_once(), cli_listen_once(&client, &ap).await, "listen_once");
            dispatch!(ap.listen.is_forever(), cli_listen_forever(&client, &ap).await, "listen_forever");
            dispatch!(ap.listen.is_tail(), cli_listen_tail(&client, &ap).await, "listen_tail");
            dispatch!(ap.listen.is_all(), cli_listen_all(&client, &ap).await, "listen_all");

            if !ap.logout.is_none() {
                match cli_logout(&client, &mut ap).await {
                    Ok(ref _n) => debug!("cli_logout successful"),
                    Err(e) => {
                        error!("Error: cli_logout reported {}", e);
                        errcount += 1;
                        result = Err(e);
                    }
                };
            }
        }
        Err(e) => {
            info!(
                "Most operations will be skipped because you don't have a valid client connection."
            );
            error!("Error: {}", e);
            errcount += 1;
            result = Err(e);
            if !ap.logout.is_none() {
                match logout_local(&ap) {
                    Ok(ref _n) => debug!("logout_local successful"),
                    Err(e) => {
                        error!("Error: logout_local reported {}", e);
                        errcount += 1;
                        result = Err(e);
                    }
                };
            };
        }
    }
    // Allow the SQLite connection pool (deadpool) to clean up gracefully
    // before the tokio runtime shuts down. The pool's Drop impl spawns
    // spawn_blocking tasks to close SQLite connections; give them time.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let plural = if errcount == 1 { "" } else { "s" };
    if errcount > 0 {
        error!("Encountered {} error{}.", errcount, plural);
    } else {
        debug!("Encountered {} error{}.", errcount, plural);
    }
    debug!("Good bye");
    result
}

/// Future test cases will be put here
#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! aw {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }

    #[test]
    fn test_usage() {
        assert_eq!(usage(), ());
    }

    #[test]
    fn test_help() {
        assert_eq!(help(), ());
    }

    #[test]
    fn test_manual() {
        assert_eq!(manual(), ());
    }

    #[test]
    fn test_readme() {
        assert_eq!(aw!(readme()), ());
    }

    #[test]
    fn test_version() {
        assert_eq!(version(Output::Text), ());
        assert_eq!(version(Output::Json), ());
    }

    #[tokio::test]
    async fn test_version_check() {
        assert_eq!(version_check().await, ());
    }

    #[test]
    fn test_contribute() {
        assert_eq!(contribute(), ());
    }
}
