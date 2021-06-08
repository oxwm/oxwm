use crate::util::*;
use crate::OxWM;
use crate::Result;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

use thiserror::Error;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;

/// Type of actions that may be triggered by keypresses. The `Window` argument
/// is the currently-focused window.
type Action<Conn> = fn(&mut OxWM<Conn>, xproto::Window) -> crate::Result<()>;

/// Bespoke `ModMask` type so that we can have a `Deserialize` instance.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum ModMask {
    Shift,
    Lock,
    Control,
    Mod1,
    Mod2,
    Mod3,
    Mod4,
    Mod5,
}

impl From<ModMask> for xproto::ModMask {
    fn from(m: ModMask) -> Self {
        match m {
            ModMask::Shift => xproto::ModMask::SHIFT,
            ModMask::Lock => xproto::ModMask::LOCK,
            ModMask::Control => xproto::ModMask::CONTROL,
            ModMask::Mod1 => xproto::ModMask::M1,
            ModMask::Mod2 => xproto::ModMask::M2,
            ModMask::Mod3 => xproto::ModMask::M3,
            ModMask::Mod4 => xproto::ModMask::M4,
            ModMask::Mod5 => xproto::ModMask::M5,
        }
    }
}

/// Focus model.
#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusModel {
    /// Click to focus.
    Click,
    /// Focus follows mouse.
    Autofocus,
}

/// Type of "raw" configs, straight from the source.
#[derive(PartialEq, Eq, Clone, Debug, Deserialize, Serialize)]
struct RawConfig {
    /// Startup programs.
    startup: Option<Vec<String>>,
    /// Global modifier mask.
    mod_mask: Option<ModMask>,
    /// Focus model.
    focus_model: Option<FocusModel>,
    /// Keybinds.
    keybinds: Option<HashMap<String, String>>,
}

/// Type of OxWM configs. Has to be parameterized by the connection type,
/// because Rust doesn't have higher-rank types yet.
#[derive(Clone)]
pub(crate) struct Config<Conn> {
    /// Startup programs.
    pub(crate) startup: Vec<String>,
    /// Global modifier mask.
    pub(crate) mod_mask: xproto::ModMask,
    /// Focus model.
    pub(crate) focus_model: FocusModel,
    /// Keybinds.
    pub(crate) keybinds: HashMap<xproto::Keycode, Action<Conn>>,
}

/// An error indicating that we can't find the user's config directory.
#[derive(
    PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, Error, Deserialize, Serialize,
)]
#[error("unsupported platform (I don't know where to look for your config file)")]
pub(crate) struct UnsupportedPlatformError;

impl<Conn> Config<Conn> {
    /// Load the config file, or return a default config object if there is no
    /// config file.
    pub(crate) fn load() -> Result<Self>
    where
        Conn: Connection,
    {
        // TODO Will this work on proper Unix (e.g., BSD)? We should probably
        // make sure it works on Unix.
        let mut path = dirs::config_dir().ok_or(UnsupportedPlatformError)?;
        path.push("oxwm");
        path.push("config.toml");
        Self::from_path(&path)
    }

    /// Load a specified config file.
    fn from_path(path: &Path) -> Result<Self>
    where
        Conn: Connection,
    {
        let s = fs::read_to_string(path)?;
        Self::from_str(&s)
    }

    /// Parse a string directly.
    fn from_str(s: &str) -> Result<Self>
    where
        Conn: Connection,
    {
        let ret = toml::from_str(s)?;
        Ok(ret)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Error)]
enum ConfigError {
    #[error("Error finding keysym for: {0}")]
    KeysymError(String),
    #[error("Error finding keycode for: {0}")]
    KeycodeError(xproto::Keysym),
    #[error("Invalid action: {0}")]
    InvalidAction(String),
}
use ConfigError::*;

impl<Conn> TryFrom<RawConfig> for Config<Conn>
where
    Conn: Connection,
{
    type Error = Box<dyn Error>;
    fn try_from(raw: RawConfig) -> Result<Self> {
        let startup = raw.startup.unwrap_or_default();
        let mod_mask = raw.mod_mask.unwrap_or(ModMask::Mod4).into();
        let focus_model = raw.focus_model.unwrap_or(FocusModel::Click);
        let mut keybinds = HashMap::new();
        for (key_name, action_name) in raw.keybinds.unwrap_or_default() {
            let keycode = match keysym_from_name(&key_name) {
                None => Err(KeysymError(key_name)),
                Some(key_sym) => match keycode_from_keysym(key_sym) {
                    None => Err(KeycodeError(key_sym)),
                    Some(key_code) => Ok(key_code),
                },
            }?;
            let action: Result<Action<Conn>> = match action_name.as_str() {
                "kill" => Ok(OxWM::kill_focused_client),
                "quit" => Ok(OxWM::poison),
                _ => Err(Box::new(InvalidAction(action_name.clone()))),
            };
            keybinds.insert(keycode, action?);
        }
        Ok(Self {
            startup,
            mod_mask,
            focus_model,
            keybinds,
        })
    }
}

impl<'de, Conn> Deserialize<'de> for Config<Conn>
where
    Conn: Connection,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawConfig::deserialize(deserializer)?;
        Self::try_from(raw).map_err(|config_error| {
            <D::Error as serde::de::Error>::custom(format!("{}", config_error))
        })
    }
}
