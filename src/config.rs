use crate::OxWM;
use crate::Result;

use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::{collections::HashMap, num::ParseIntError};

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeStruct;

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
    Any,
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
            ModMask::Any => xproto::ModMask::ANY,
        }
    }
}

impl ModMask {
    fn from(xm: &xproto::ModMask) -> Self {
        match xm {
            &xproto::ModMask::SHIFT => ModMask::Shift,
            &xproto::ModMask::LOCK => ModMask::Lock,
            &xproto::ModMask::CONTROL => ModMask::Control,
            &xproto::ModMask::M1 => ModMask::Mod1,
            &xproto::ModMask::M2 => ModMask::Mod2,
            &xproto::ModMask::M3 => ModMask::Mod3,
            &xproto::ModMask::M4 => ModMask::Mod4,
            &xproto::ModMask::M5 => ModMask::Mod5,
            _ => ModMask::Any,
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
    startup: Option<Vec<String>>,
    mod_mask: Option<ModMask>,
    focus_model: Option<FocusModel>,
    keybinds: Option<HashMap<String, String>>,
}

/// Type of OxWM configs. Has to be parameterized by the connection type,
/// because Rust doesn't have higher-rank types yet.
#[derive(Clone)]
pub(crate) struct Config<Conn> {
    pub(crate) startup: Vec<String>,
    pub(crate) mod_mask: xproto::ModMask,
    pub(crate) focus_model: FocusModel,
    pub(crate) keybinds: HashMap<xproto::Keycode, Action<Conn>>,
}

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
        // TODO Will this work on Unix (e.g., BSD)? We should probably make sure
        // it works on Unix.
        let mut path = dirs::config_dir().ok_or(UnsupportedPlatformError)?;
        path.push("oxwm");
        path.push("config.toml");
        Self::from_path(&path).or_else(|_| {
            log::info!("Applying default configuration.");
            let default_config = Self::new()?;
            let strc = toml::to_string(&default_config);
            log::debug!("Heeere's jonny!\nvvv\n{}\n^^^",strc.unwrap());

            Ok(default_config)
        })
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

    /// Instantiate a default config which opens an xterm at startup, changes
    /// focus on mouse click, terminates programs with Mod4 + w, and exits with Mod4 + Q.
    fn new() -> Result<Self>
    where
        Conn: Connection,
    {
        let startup: Vec<String> = vec!["xterm".to_string()];
        let mod_mask = ModMask::Mod4.into();
        let focus_model = FocusModel::Click;
        let mut keybinds: HashMap<xproto::Keycode, Action<Conn>> = HashMap::new();
        keybinds.insert(24, OxWM::poison); //Q
        keybinds.insert(25, OxWM::kill_focused_client); //W

        Ok(Self {
            startup,
            mod_mask,
            focus_model,
            keybinds,
        })
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Error)]
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
        let focus_model = raw.focus_model.unwrap_or(FocusModel::Click);
        let mut keybinds = HashMap::new();
        for (keycode, action_name) in raw.keybinds.unwrap_or_default() {
            let keycode = keycode.parse::<u8>().map_err(KeycodeError)?;
            let action: Result<Action<Conn>> = match action_name.as_str() {
                "kill" => Ok(OxWM::kill_focused_client),
                "quit" => Ok(OxWM::poison),
                _ => Err(Box::new(ActionError(action_name.clone()))),
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
        Self::try_from(raw).map_err(|action_name| {
            <D::Error as serde::de::Error>::custom(format!("unknown action {}", action_name))
        })
    }
}

impl<Conn> Serialize for Config<Conn>
where
    Conn: Connection,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut output = serializer.serialize_struct("Config", 3)?;
        output.serialize_field("startup", &self.startup);
        output.serialize_field("mod_mask", &ModMask::from(&self.mod_mask));
        output.serialize_field("focus_model", &self.focus_model);
        //output.serialize_field("keybinds", &self.keybinds);
        output.end()
    }
}
