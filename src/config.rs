use crate::action;
use crate::action::Action;
use crate::OxWM;
use crate::Result;

use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::{collections::HashMap, num::ParseIntError};

use serde::Deserialize;
use serde::Deserializer;

use thiserror::Error;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;

/// Bespoke `ModMask` type so that we can have a `Deserialize` instance.
#[derive(Deserialize)]
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

/// Type of "raw" configs, straight from the source.
#[derive(Deserialize)]
struct RawConfig {
    startup: Option<Vec<String>>,
    mod_mask: Option<ModMask>,
    keybinds: Option<HashMap<String, String>>,
}

/// Type of OxWM configs. Has to be parameterized by the connection type,
/// because Rust doesn't have higher-rank types yet.
pub(crate) struct Config<Conn> {
    pub(crate) startup: Vec<String>,
    pub(crate) mod_mask: xproto::ModMask,
    pub(crate) keybinds: HashMap<xproto::Keycode, Action<Conn>>,
}

#[derive(Debug, Error)]
#[error("unsupported platform (I don't know where to look for your config file)")]
pub(crate) struct UnsupportedPlatformError;

impl<Conn> Config<Conn> {
    /// Load the config file, or return a default config object if there is no
    /// config file.
    pub(crate) fn load() -> Result<Self>
    where
        Conn: Connection,
    {
        // TODO Will this work on Unix (e.g., BSD)? We should probably make sure
        // it works on Unix.
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
        toml::from_str(s).map_err(|e| Box::new(e) as Box<dyn Error>)
    }
}

#[derive(Debug, Error)]
enum ConfigError {
    #[error("error while parsing keycode: {0:}")]
    KeycodeError(ParseIntError),
    #[error("invalid action: {0}")]
    ActionError(String),
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
        let mut keybinds = HashMap::new();
        for (keycode, action_name) in raw.keybinds.unwrap_or_default() {
            let keycode = keycode.parse::<u8>().map_err(KeycodeError)?;
            let action: Result<Action<Conn>> = match action_name.as_str() {
                "kill" => Ok(|oxwm| OxWM::kill_focused_client(&*oxwm)),
                "quit" => Ok(action::quit),
                _ => Err(Box::new(ActionError(action_name.clone()))),
            };
            keybinds.insert(keycode, action?);
        }
        Ok(Self {
            startup,
            mod_mask,
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
        Self::try_from(raw).map_err(|action_name| {
            <D::Error as serde::de::Error>::custom(format!("unknown action {}", action_name))
        })
    }
}
