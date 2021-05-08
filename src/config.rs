use crate::action;
use crate::action::Action;

use dirs;

use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::{collections::HashMap, num::ParseIntError};

use serde::Deserialize;
use serde::Deserializer;

use thiserror::Error;

use x11rb::protocol::xproto;

/// Bespoke ModMask type so that we can have a `Deserialize` instance.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum ModMask {
    SHIFT,
    LOCK,
    CONTROL,
    MOD1,
    MOD2,
    MOD3,
    MOD4,
    MOD5,
}

impl Into<xproto::ModMask> for ModMask {
    fn into(self) -> xproto::ModMask {
        match self {
            Self::SHIFT => xproto::ModMask::SHIFT,
            Self::LOCK => xproto::ModMask::LOCK,
            Self::CONTROL => xproto::ModMask::CONTROL,
            Self::MOD1 => xproto::ModMask::M1,
            Self::MOD2 => xproto::ModMask::M2,
            Self::MOD3 => xproto::ModMask::M3,
            Self::MOD4 => xproto::ModMask::M4,
            Self::MOD5 => xproto::ModMask::M5,
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
pub struct Config<Conn> {
    pub startup: Vec<String>,
    pub mod_mask: xproto::ModMask,
    pub keybinds: HashMap<xproto::Keycode, Action<Conn>>,
}

#[derive(Debug, Error)]
#[error("unsupported platform (I don't know where to look for your config file)")]
pub struct UnsupportedPlatformError;

impl<Conn> Config<Conn> {
    /// Load the config file, or return a default config object if there is no
    /// config file.
    pub fn load() -> Result<Self, Box<dyn Error>> {
        // TODO Will this work on Unix? We should probably make sure it works on
        // Unix.
        let mut path = dirs::config_dir().ok_or(UnsupportedPlatformError)?;
        path.push("oxwm");
        path.push("config.toml");
        Self::from_path(&path)
    }

    /// Load a specified config file.
    fn from_path(path: &Path) -> Result<Self, Box<dyn Error>> {
        let s = fs::read_to_string(path)?;
        Self::from_str(&s)
    }

    /// Parse a string directly.
    fn from_str(s: &str) -> Result<Self, Box<dyn Error>> {
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

impl<Conn> TryFrom<RawConfig> for Config<Conn> {
    type Error = ConfigError;
    fn try_from(raw: RawConfig) -> Result<Self, Self::Error> {
        let startup = raw.startup.unwrap_or(Vec::new());
        let mod_mask = raw.mod_mask.unwrap_or(ModMask::MOD4).into();
        let mut keybinds = HashMap::new();
        for (keycode, action_name) in raw.keybinds.unwrap_or(HashMap::new()) {
            let keycode = u8::from_str_radix(&keycode, 10).map_err(KeycodeError)?;
            let action: Result<Action<Conn>, _> = match action_name.as_str() {
                "quit" => Ok(action::quit),
                _ => Err(ActionError(action_name.clone())),
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

impl<'de, Conn> Deserialize<'de> for Config<Conn> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawConfig::deserialize(deserializer)?;
        Self::try_from(raw).map_err(|action_name| {
            <D::Error as serde::de::Error>::custom(format!("unknown action {}", action_name))
        })
    }
}
