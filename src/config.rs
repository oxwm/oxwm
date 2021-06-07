use crate::util::*;
use crate::OxWM;
use crate::Result;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::ser::SerializeStruct;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

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
        match *xm {
            xproto::ModMask::SHIFT => ModMask::Shift,
            xproto::ModMask::LOCK => ModMask::Lock,
            xproto::ModMask::CONTROL => ModMask::Control,
            xproto::ModMask::M1 => ModMask::Mod1,
            xproto::ModMask::M2 => ModMask::Mod2,
            xproto::ModMask::M3 => ModMask::Mod3,
            xproto::ModMask::M4 => ModMask::Mod4,
            xproto::ModMask::M5 => ModMask::Mod5,
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
    pub(crate) keybind_names: HashMap<xproto::Keycode, String>,
}

#[derive(
    PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, Error, Deserialize, Serialize,
)]
#[error("unsupported platform (I don't know where to look for your config file)")]
pub(crate) struct UnsupportedPlatformError;

#[derive(Clone, Copy, Debug, Error, Deserialize, Serialize)]
#[error("Unable to create oxwm's configuration directory.")]
pub(crate) struct CannotMakeConfigDirError;

#[derive(Clone, Copy, Debug, Error, Deserialize, Serialize)]
#[error("Unable to access your user's configuration directory.")]
pub(crate) struct ConfigDirAccessError;

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

    /// Instantiate a default config which opens an xterm at startup, changes
    /// focus on mouse click, terminates programs with Mod4 + w, and exits with Mod4 + Q.
    pub fn new() -> Self
    where
        Conn: Connection,
    {
        let startup: Vec<String> = vec!["xterm".to_string()];
        let mod_mask = ModMask::Mod4.into();
        let focus_model = FocusModel::Click;
        let mut keybinds: HashMap<xproto::Keycode, Action<Conn>> = HashMap::new();
        let mut keybind_names: HashMap<xproto::Keycode, String> = HashMap::new();
        keybinds.insert(24, OxWM::poison); //Q
        keybind_names.insert(24, "quit".to_string());
        keybinds.insert(25, OxWM::kill_focused_client); //W
        keybind_names.insert(25, "kill".to_string());

        Self {
            startup,
            mod_mask,
            focus_model,
            keybinds,
            keybind_names,
        }
    }

    /// Write the config in .toml format to the default location:
    /// `<config directory>/oxwm/config.toml`
    /// Jwhere `config_directory` is the location returned by `dirs::config_dir()`.
    /// Will create the `oxwm` directory if needed, will not create `config_directory`
    pub fn save(&self) -> Result<()>
    where
        Conn: Connection,
    {
        //TODO Need to ensure config_dir also works on unix platforms.
        let mut path = dirs::config_dir().ok_or(UnsupportedPlatformError)?;

        //Fail if user configuration directory is not usable.
        //TODO do we want to actually make this directory if it is missing?
        if !path.is_dir() {
            return Err(Box::new(ConfigDirAccessError));
        };

        //Check if oxwm directory is usable, attempt to create it if not.
        path.push("oxwm");
        if !path.is_dir() {
            if path.exists() {
                //Something is there, but we cannot access it or it isn't a directory.
                return Err(Box::new(CannotMakeConfigDirError));
            } else {
                fs::create_dir(&path)?;
                log::info!("Created directory {}.", path.display());
            }
        }

        //Create or overwrite existing config.toml
        path.push("config.toml");
        fs::write(&path, toml::to_string(&self)?)?;
        log::info!("Saved configuration file to {}.", path.display());

        Ok(())
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
        //let mut keybind_names = HashMap::new();
        let keybind_names = raw.keybinds.clone(); //Retain for later serialization
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
            //keybind_names.insert(keycode, action_name);
        }
        Ok(Self {
            startup,
            mod_mask,
            focus_model,
            keybinds,
            keybind_names,
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

impl<Conn> Serialize for Config<Conn>
where
    Conn: Connection,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut output = serializer.serialize_struct("Config", 4)?;
        output.serialize_field("startup", &self.startup)?;
        output.serialize_field("mod_mask", &ModMask::from(&self.mod_mask))?;
        output.serialize_field("focus_model", &self.focus_model)?;

        // Omit serializing self.keybinds directly: not trivial to convert from
        // Action<Conn> to the strings they would be named by in a config.toml file.
        // Use the corresponding strings mapped in self.keybind_names instead.
        // Additionally toml::ser requires that keys for map types be Strings
        // instead of integer types like xproto::Keycode. Convert to String first.
        let keybind_names_by_string: HashMap<String, &String> = self
            .keybind_names
            .iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        output.serialize_field("keybinds", &keybind_names_by_string)?;
        output.end()
    }
}
