use crate::util::*;
use crate::OxWM;
use crate::Result;

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

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
    #[serde(skip_deserializing)]
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

/// Type of OxWM configs. Has to be parameterized by the connection type,
/// because Rust doesn't have higher-rank types yet.
#[derive(Clone, Deserialize, Serialize)]
#[serde(default = "Config::new_core")]
pub(crate) struct Config<Conn> {
    pub(crate) startup: Vec<String>,

    #[serde(deserialize_with = "deserialize_xproto_modmask")]
    #[serde(serialize_with = "serialize_xproto_modmask")]
    pub(crate) mod_mask: xproto::ModMask,

    pub(crate) focus_model: FocusModel,

    #[serde(skip)]
    pub(crate) keybinds: HashMap<xproto::Keycode, Action<Conn>>,

    #[serde(rename = "keybinds")]
    pub(crate) keybind_names: HashMap<String, String>,
}

/// Deserialize an xproto::ModMask value by first deserializing into a
/// Config::ModMask and converting from that to an xproto::ModMask.
fn deserialize_xproto_modmask<'de, D>(
    deserializer: D,
) -> std::result::Result<xproto::ModMask, D::Error>
where
    D: Deserializer<'de>,
{
    let modm = ModMask::deserialize(deserializer)?;
    Ok(xproto::ModMask::from(modm))
}

/// Serialize an xproto::ModMask by first converting it to a Config::ModMask
/// and serializing that enum instead.
fn serialize_xproto_modmask<S>(
    source: &xproto::ModMask,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let modm = ModMask::from(source);
    modm.serialize(serializer)
}

#[derive(
    PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, Error, Deserialize, Serialize,
)]
#[error("Unsupported platform (I don't know where to look for your config file)")]
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
        let mut ret: Self = toml::from_str(s).map_err(|e| Box::new(e) as Box<dyn Error>)?;
        ret.translate_keybinds()?;
        Ok(ret)
    }

    /// Populate `self.keybinds` with Keycodes and `Action<Conn>` fn pointers
    /// that match the Keysyms and action names found in `self.keybind_names`.
    fn translate_keybinds(&mut self) -> Result<()>
    where
        Conn: Connection,
    {
        for (key_name, action_name) in &self.keybind_names {
            let keycode = match keysym_from_name(&key_name) {
                None => Err(KeysymError(key_name.clone())),
                Some(key_sym) => match keycode_from_keysym(key_sym) {
                    None => Err(KeycodeError(key_name.clone(), key_sym)),
                    Some(key_code) => Ok(key_code),
                },
            }?;
            let action: std::result::Result<Action<Conn>, ConfigError> = match action_name.as_str()
            {
                "quit" => Ok(OxWM::poison),
                "kill" => Ok(OxWM::kill_focused_client),
                _ => Err(InvalidAction(action_name.clone())),
            };

            self.keybinds.insert(keycode, action?);
        }
        Ok(())
    }

    /// Instantiate a default config which opens an xterm at startup, changes
    /// focus on mouse click, kills windows with Mod4 + w, and exits with Mod4 + Q.
    pub fn new() -> Result<Self>
    where
        Conn: Connection,
    {
        let mut ret = Config::new_core();
        ret.translate_keybinds()?;
        Ok(ret)
    }

    /// Instantiates a Config with default settings, but does NOT attempt to bind
    /// Keycodes and `Action<Conn>` fn pointers into the `keybinds` field.
    /// Used by `Config::new`. Also used by derive[(Serialize)] on Config to fill in
    /// default values for any fields that aren't specified in the existing
    /// Config.toml file.
    /// The serde derive macros don't like that Serialize trait isn't specified for
    /// `x11rb::xproto::Connection`. By ommitting any references to `Conn`/`Connection`
    /// in this function serde is allowed to serialize/deserialize Config's directly.
    /// Callers to this function are expected to call the `translate_keybinds()`
    /// function of the returned Config to populate the keybind field.
    fn new_core() -> Self {
        let startup: Vec<String> = vec!["xterm".to_string()];
        let mod_mask = ModMask::Mod4.into();
        let focus_model = FocusModel::Click;

        // Deliberately left unpopulated, callers are expected to call the new
        // Config object's translate_keybinds method to populate keybinds before use.
        let keybinds = HashMap::new();
        let mut keybind_names: HashMap<String, String> = HashMap::new();
        keybind_names.insert("q".to_string(), "quit".to_string());
        keybind_names.insert("w".to_string(), "kill".to_string());
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
    /// where `config_directory` is the location returned by `dirs::config_dir()`.
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
pub(crate) enum ConfigError {
    #[error("Unrecodgnized key \"{0}\" in your Config.toml")]
    KeysymError(String),
    #[error("X11 server does not have a Keycode assigned for \"{0}\" (Keysym: {1:#x})\nThis key may not be available in your current keyboard layout.")]
    KeycodeError(String, xproto::Keysym),
    #[error("Invalid action \"{0}\" found in your Config.toml")]
    InvalidAction(String),
}
use ConfigError::*;
