// types.rs — Enums, Error, Credentials, and related types

use clap::ValueEnum;
use matrix_sdk::{
    ruma::{OwnedDeviceId, OwnedUserId},
    SessionMeta,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug};
use std::fs::{self, File};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;
use tracing::info;
use url::Url;

/// The enumerator for Errors
#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Custom(&'static str),

    #[error("No valid home directory path")]
    NoHomeDirectory,

    #[error("Not logged in")]
    NotLoggedIn,

    #[error("Invalid Room")]
    InvalidRoom,

    #[error("Homeserver Not Set")]
    HomeserverNotSet,

    #[error("Invalid File")]
    InvalidFile,

    #[error("Login Failed")]
    LoginFailed,

    #[error("Verify Failed or Partially Failed")]
    VerifyFailed,

    #[error("Bootstrap Failed")]
    BootstrapFailed,

    #[error("Already logged in")]
    LoginUnnecessary,

    #[error("Send Failed")]
    SendFailed,

    #[error("Listen Failed")]
    ListenFailed,

    #[error("Create Room Failed")]
    CreateRoomFailed,

    #[error("Leave Room Failed")]
    LeaveRoomFailed,

    #[error("Forget Room Failed")]
    ForgetRoomFailed,

    #[error("Invite Room Failed")]
    InviteRoomFailed,

    #[error("Join Room Failed")]
    JoinRoomFailed,

    #[error("Ban Room Failed")]
    BanRoomFailed,

    #[error("Unban Room Failed")]
    UnbanRoomFailed,

    #[error("Kick Room Failed")]
    KickRoomFailed,

    #[error("Resolve Room Alias Failed")]
    ResolveRoomAliasFailed,

    #[error("Enable Encryption Failed")]
    EnableEncryptionFailed,

    #[error("Room Get Visibility Failed")]
    RoomGetVisibilityFailed,

    #[error("Room Get State Failed")]
    RoomGetStateFailed,

    #[error("JoinedMembersFailed")]
    JoinedMembersFailed,

    #[error("Delete Device Failed")]
    DeleteDeviceFailed,

    #[error("Get Avatar Failed")]
    GetAvatarFailed,

    #[error("Set Avatar Failed")]
    SetAvatarFailed,

    #[error("Get Avatar URL Failed")]
    GetAvatarUrlFailed,

    #[error("Set Avatar URL Failed")]
    SetAvatarUrlFailed,

    #[error("Unset Avatar URL Failed")]
    UnsetAvatarUrlFailed,

    #[error("Get Displayname Failed")]
    GetDisplaynameFailed,

    #[error("Set Displayname Failed")]
    SetDisplaynameFailed,

    #[error("Get Profile Failed")]
    GetProfileFailed,

    #[error("Get Masterkey Failed")]
    GetMasterkeyFailed,

    #[error("Restoring Login Failed")]
    RestoreLoginFailed,

    #[error("Media Upload Failed")]
    MediaUploadFailed,

    #[error("Media Download Failed")]
    MediaDownloadFailed,

    #[error("Media Delete Failed")]
    MediaDeleteFailed,

    #[error("MXC TO HTTP Failed")]
    MediaMxcToHttpFailed,

    #[error("Event Send Failed")]
    EventSendFailed,

    #[error("Import Keys Failed")]
    ImportKeysFailed,

    #[error("Export Keys Failed")]
    ExportKeysFailed,

    #[error("Get OpenID Token Failed")]
    GetOpenIdTokenFailed,

    #[error("Set Device Name Failed")]
    SetDeviceNameFailed,

    #[error("Set Presence Failed")]
    SetPresenceFailed,

    #[error("Get Presence Failed")]
    GetPresenceFailed,

    #[error("Room Set Alias Failed")]
    RoomSetAliasFailed,

    #[error("Room Delete Alias Failed")]
    RoomDeleteAliasFailed,

    #[error("Discovery Info Failed")]
    DiscoveryInfoFailed,

    #[error("Login Info Failed")]
    LoginInfoFailed,

    #[error("Content Repository Config Failed")]
    ContentRepositoryConfigFailed,

    #[error("Delete MXC Before Failed")]
    DeleteMxcBeforeFailed,

    #[error("Get Client Info Failed")]
    GetClientInfoFailed,

    #[error("Room Invites Failed")]
    RoomInvitesFailed,

    #[error("Room Redact Failed")]
    RoomRedactFailed,

    #[error("Has Permission Failed")]
    HasPermissionFailed,

    #[error("Joined DM Rooms Failed")]
    JoinedDmRoomsFailed,

    #[error("REST API Call Failed")]
    RestFailed,

    #[error("Invalid Client Connection")]
    InvalidClientConnection,

    #[error("Unknown CLI parameter")]
    UnknownCliParameter,

    #[error("Unsupported CLI parameter: {0}")]
    UnsupportedCliParameter(&'static str),

    #[error("Missing Room")]
    MissingRoom,

    #[error("Missing User")]
    MissingUser,

    #[error("Missing Password")]
    MissingPassword,

    #[error("Missing CLI parameter")]
    MissingCliParameter,

    #[error("Not Implemented Yet")]
    NotImplementedYet,

    #[error("No Credentials Found")]
    NoCredentialsFound,

    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    Matrix(#[from] matrix_sdk::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Http(#[from] matrix_sdk::HttpError),
}

/// Function to create custom error messages on the fly with static text
#[allow(dead_code)]
impl Error {
    pub(crate) fn custom<T>(message: &'static str) -> Result<T, Error> {
        Err(Error::Custom(message))
    }
}

/// Enumerator used for --login option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Login {
    /// None: no login specified, don't login
    #[default]
    None,
    /// Password: login with password
    Password,
    /// AccessToken: login with access-token
    AccessToken,
    /// SSO: login with SSO, single-sign on
    Sso,
}

impl Login {
    pub fn is_password(&self) -> bool {
        self == &Self::Password
    }
    pub fn is_access_token(&self) -> bool {
        self == &Self::AccessToken
    }
    pub fn is_none(&self) -> bool {
        self == &Self::None
    }
}

impl FromStr for Login {
    type Err = ();
    fn from_str(src: &str) -> Result<Login, ()> {
        match src.to_lowercase().trim() {
            "none" => Ok(Login::None),
            "password" => Ok(Login::Password),
            "access_token" | "access-token" | "accesstoken" => Ok(Login::AccessToken),
            "sso" => Ok(Login::Sso),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Login {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --sync option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Sync {
    /// Turns syncing off for sending operations to improve performance
    Off,
    /// full: the default value
    #[default]
    Full,
}

impl Sync {
    pub fn is_off(&self) -> bool {
        self == &Self::Off
    }
    pub fn is_full(&self) -> bool {
        self == &Self::Full
    }
}

impl FromStr for Sync {
    type Err = ();
    fn from_str(src: &str) -> Result<Sync, ()> {
        match src.to_lowercase().trim() {
            "off" => Ok(Sync::Off),
            "full" => Ok(Sync::Full),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Sync {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --set-presence option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Presence {
    /// Online
    #[default]
    Online,
    /// Offline
    Offline,
    /// Unavailable
    Unavailable,
}

impl fmt::Display for Presence {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Presence::Online => write!(f, "online"),
            Presence::Offline => write!(f, "offline"),
            Presence::Unavailable => write!(f, "unavailable"),
        }
    }
}

/// Enumerator used for --version option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Version {
    /// Check if there is a newer version available
    #[default]
    Check,
}

impl FromStr for Version {
    type Err = ();
    fn from_str(src: &str) -> Result<Version, ()> {
        match src.to_lowercase().trim() {
            "check" => Ok(Version::Check),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --verify option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Verify {
    /// None: option not used, no verification done
    #[default]
    None,
    /// ManualDevice: manual device verification
    ManualDevice,
    /// ManualUser: manual user verification
    ManualUser,
    /// Emoji: verify via emojis as the recipient
    Emoji,
    /// Emoji: verify via emojis as the initiator
    EmojiReq,
}

impl Verify {
    pub fn is_none(&self) -> bool {
        self == &Self::None
    }
    pub fn is_manual_device(&self) -> bool {
        self == &Self::ManualDevice
    }
    pub fn is_manual_user(&self) -> bool {
        self == &Self::ManualUser
    }
    pub fn is_emoji(&self) -> bool {
        self == &Self::Emoji
    }
    pub fn is_emoji_req(&self) -> bool {
        self == &Self::EmojiReq
    }
}

impl FromStr for Verify {
    type Err = ();
    fn from_str(src: &str) -> Result<Verify, ()> {
        match src.to_lowercase().trim() {
            "none" => Ok(Verify::None),
            "manual-device" => Ok(Verify::ManualDevice),
            "manual-user" => Ok(Verify::ManualUser),
            "emoji" => Ok(Verify::Emoji),
            "emoji-req" => Ok(Verify::EmojiReq),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Verify {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --logout option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Logout {
    /// None: Log out nowhere, don't do anything, default
    #[default]
    None,
    /// Me: Log out from the currently used device
    Me,
    /// All: Log out from all devices of the user
    All,
}

impl Logout {
    pub fn is_none(&self) -> bool {
        self == &Self::None
    }
    pub fn is_me(&self) -> bool {
        self == &Self::Me
    }
    pub fn is_all(&self) -> bool {
        self == &Self::All
    }
}

impl FromStr for Logout {
    type Err = ();
    fn from_str(src: &str) -> Result<Logout, ()> {
        match src.to_lowercase().trim() {
            "none" => Ok(Logout::None),
            "me" => Ok(Logout::Me),
            "all" => Ok(Logout::All),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Logout {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --listen (--tail) option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Listen {
    /// Never: Indicates to not listen, default
    #[default]
    Never,
    /// Once: Indicates to listen once in *all* rooms and then continue
    Once,
    /// Forever: Indicates to listen forever in *all* rooms, until process is killed manually.
    Forever,
    /// Tail: Indicates to get the last N messages from the specified room(s) and then continue
    Tail,
    /// All: Indicates to get *all* the messages from from the specified room(s) and then continue
    All,
}

impl Listen {
    pub fn is_never(&self) -> bool {
        self == &Self::Never
    }
    pub fn is_once(&self) -> bool {
        self == &Self::Once
    }
    pub fn is_forever(&self) -> bool {
        self == &Self::Forever
    }
    pub fn is_tail(&self) -> bool {
        self == &Self::Tail
    }
    pub fn is_all(&self) -> bool {
        self == &Self::All
    }
}

impl FromStr for Listen {
    type Err = ();
    fn from_str(src: &str) -> Result<Listen, ()> {
        match src.to_lowercase().trim() {
            "never" => Ok(Listen::Never),
            "once" => Ok(Listen::Once),
            "forever" => Ok(Listen::Forever),
            "tail" => Ok(Listen::Tail),
            "all" => Ok(Listen::All),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Listen {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --log-level option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum LogLevel {
    /// None: not set, default.
    #[default]
    None,
    /// Error: Indicates to print only errors
    Error,
    /// Warn: Indicates to print warnings and errors
    Warn,
    /// Info: Indicates to to print info, warn and errors
    Info,
    /// Debug: Indicates to to print debug and the rest
    Debug,
    /// Trace: Indicates to to print everything
    Trace,
}

impl LogLevel {
    pub fn is_none(&self) -> bool {
        self == &Self::None
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --output option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum Output {
    /// Text: Indicates to print human readable text, default
    #[default]
    Text,
    /// Json: Indicates to print output in Json format
    Json,
    /// Json Max: Indicates to print the maximum amount of output in Json format
    JsonMax,
    /// Json Spec: Indicates to print output in Json format, but only data that is according to Matrix Specifications
    JsonSpec,
}

impl Output {
    pub fn is_text(&self) -> bool {
        self == &Self::Text
    }
}

impl FromStr for Output {
    type Err = ();
    fn from_str(src: &str) -> Result<Output, ()> {
        match src.to_lowercase().replace('-', "_").trim() {
            "text" => Ok(Output::Text),
            "json" => Ok(Output::Json),
            "jsonmax" | "json_max" => Ok(Output::JsonMax),
            "jsonspec" | "json_spec" => Ok(Output::JsonSpec),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Enumerator used for --download-media-name option
#[derive(Clone, Debug, Copy, PartialEq, Default, ValueEnum)]
pub enum DownloadMediaName {
    /// Source: Use the file name from the source
    Source,
    /// Clean: Use a cleaned-up version of the file name
    #[default]
    Clean,
    /// EventId: Use the event id as file name
    EventId,
    /// Time: Use the timestamp as file name
    Time,
}

/// A struct for the credentials. These will be serialized into JSON
/// and written to the credentials.json file for permanent storage and
/// future access.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub homeserver: Url,
    pub user_id: OwnedUserId,
    pub access_token: String,
    pub device_id: OwnedDeviceId,
    pub room_id: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

impl AsRef<Credentials> for Credentials {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl Credentials {
    /// Default constructor
    pub fn new(
        homeserver: Url,
        user_id: OwnedUserId,
        access_token: String,
        device_id: OwnedDeviceId,
        room_id: String,
        refresh_token: Option<String>,
    ) -> Self {
        Self {
            homeserver,
            user_id,
            access_token,
            device_id,
            room_id,
            refresh_token,
        }
    }

    /// Constructor for Credentials
    pub fn load(path: &Path) -> Result<Credentials, Error> {
        let reader = File::open(path)?;
        Credentials::set_permissions(&reader)?;
        let credentials: Credentials = serde_json::from_reader(reader)?;
        let mut credentialsfiltered = credentials.clone();
        credentialsfiltered.access_token = "***".to_string();
        info!("loaded credentials are: {:?}", credentialsfiltered);
        Ok(credentials)
    }

    /// Writing the credentials to a file
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        fs::create_dir_all(path.parent().ok_or(Error::NoHomeDirectory)?)?;
        let writer = File::create(path)?;
        // Build ordered JSON matching Python's format:
        // homeserver (no trailing slash), device_id, user_id, room_id, access_token
        let mut hs = self.homeserver.to_string();
        if hs.ends_with('/') {
            hs.pop();
        }
        let mut map = serde_json::Map::new();
        map.insert(
            "homeserver".to_string(),
            serde_json::Value::String(hs),
        );
        map.insert(
            "device_id".to_string(),
            serde_json::Value::String(self.device_id.to_string()),
        );
        map.insert(
            "user_id".to_string(),
            serde_json::Value::String(self.user_id.to_string()),
        );
        map.insert(
            "room_id".to_string(),
            serde_json::Value::String(self.room_id.clone()),
        );
        map.insert(
            "access_token".to_string(),
            serde_json::Value::String(self.access_token.clone()),
        );
        // Only include refresh_token if it has a value (omit null)
        if let Some(ref rt) = self.refresh_token {
            map.insert(
                "refresh_token".to_string(),
                serde_json::Value::String(rt.clone()),
            );
        }
        serde_json::to_writer_pretty(&writer, &map)?;
        Credentials::set_permissions(&writer)?;
        Ok(())
    }

    #[cfg(unix)]
    fn set_permissions(file: &File) -> Result<(), Error> {
        use std::os::unix::fs::PermissionsExt;
        let perms = file.metadata()?.permissions();
        if perms.mode() & 0o4 == 0o4 {
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn set_permissions(file: &File) -> Result<(), Error> {
        Ok(())
    }
}

/// Implements From trait for Session
impl From<Credentials> for SessionMeta {
    fn from(credentials: Credentials) -> Self {
        Self {
            user_id: credentials.user_id,
            device_id: credentials.device_id,
        }
    }
}
