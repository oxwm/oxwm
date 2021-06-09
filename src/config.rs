//! Load config files.

use crate::util::*;
use crate::OxWM;
use crate::Result;

use std::collections::HashMap;
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

/// Allow converting from a Config::Modmask to an xproto::Modmask
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

/// Allow converting from an xproto::Modmask to a Config::Modmask
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
    /// Startup programs.
    pub(crate) startup: Vec<String>,
    /// Global modifier key mask.
    #[serde(deserialize_with = "deserialize_xproto_modmask")]
    #[serde(serialize_with = "serialize_xproto_modmask")]
    pub(crate) mod_mask: xproto::ModMask,
    /// Focus model.
    pub(crate) focus_model: FocusModel,
    /// Active keybinds for running window manager.
    #[serde(skip)]
    pub(crate) keybinds: HashMap<xproto::Keycode, Action<Conn>>,
    /// Keybinds as represented in Config.toml.
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

/// An error indicating that we can't find the user's config directory.
#[derive(
    PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash, Error, Deserialize, Serialize,
)]
#[error("Unsupported platform (I don't know where to look for your config file)")]
pub(crate) struct UnsupportedPlatformError;

/// An error indicating that we couldn't make the oxwm specific directory inside the user's
/// config directory.
#[derive(Clone, Copy, Debug, Error, Deserialize, Serialize)]
#[error("Unable to create oxwm's configuration directory.")]
pub(crate) struct CannotMakeConfigDirError;

/// An error indicating that the user's config directory is missing or otherwise inaccessable.
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
        let mut ret: Self = toml::from_str(s)?;
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

/// Errors relating to finding invalid but properly formed `Config.toml` contents.
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

/// Confirm that a usable `Config` can be produced by deserializing a Config.toml file.
#[test]
fn check_deserialize() {
    // Cannot verify Config.keybinds as this requires querying an X11 server.
    let good_toml =
        "startup = [\"xterm\", \"xclock\"]\nmod_mask = \"mod3\"\nfocus_model = \"autofocus\"\n\n[keybinds]\nF4 = \"kill\"\nEscape = \"quit\"\n";
    let response: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(good_toml);
    assert!(response.is_ok());
    let a_config = response.unwrap();
    assert_eq!(a_config.startup, vec!["xterm", "xclock"]);
    assert_eq!(a_config.mod_mask, xproto::ModMask::M3);
    assert_eq!(a_config.focus_model, FocusModel::Autofocus);
    assert!(a_config.keybind_names.contains_key("F4"));
    assert_eq!(a_config.keybind_names["F4"], "kill");
    assert!(a_config.keybind_names.contains_key("Escape"));
    assert_eq!(a_config.keybind_names["Escape"], "quit");
    assert_eq!(a_config.keybind_names.len(), 2);
}

/// Confirm that the `serde` / `toml` crates fill in missing information appropriately when deserializing from
/// an incomplete Config.toml file.
#[test]
fn check_deserialize_defaults() {
    // Cannot verify Config.keybinds as this requires querying an X11 server.
    let empty_toml = "";
    let response: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(empty_toml);
    assert!(response.is_ok());
    let a_config = response.unwrap();
    assert_eq!(a_config.startup, vec!["xterm"]);
    assert_eq!(a_config.mod_mask, xproto::ModMask::M4);
    assert_eq!(a_config.focus_model, FocusModel::Click);
    assert!(a_config.keybind_names.contains_key("w"));
    assert_eq!(a_config.keybind_names["w"], "kill");
    assert!(a_config.keybind_names.contains_key("q"));
    assert_eq!(a_config.keybind_names["q"], "quit");
    assert_eq!(a_config.keybind_names.len(), 2);

    let partial_toml =
        "startup = [\"xterm\", \"xclock\"]\n[keybinds]\nF4 = \"kill\"\nEscape = \"quit\"\n";
    let response: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(partial_toml);
    assert!(response.is_ok());
    let a_config = response.unwrap();
    assert_eq!(a_config.startup, vec!["xterm", "xclock"]);
    assert_eq!(a_config.mod_mask, xproto::ModMask::M4); // from defaults
    assert_eq!(a_config.focus_model, FocusModel::Click); // from defaults
    assert!(a_config.keybind_names.contains_key("F4"));
    assert_eq!(a_config.keybind_names["F4"], "kill");
    assert!(a_config.keybind_names.contains_key("Escape"));
    assert_eq!(a_config.keybind_names["Escape"], "quit");
    assert_eq!(a_config.keybind_names.len(), 2);
}

/// Confirm that serialization via `serde` and `toml` crates produces expected results.
#[test]
fn check_serialize() {
    let good_toml =
        "startup = [\"xterm\", \"xclock\"]\nmod_mask = \"mod4\"\nfocus_model = \"click\"\n\n[keybinds]\nw = \"kill\"\nq = \"quit\"\n";
    let alternate_toml =
        "startup = [\"xterm\", \"xclock\"]\nmod_mask = \"mod4\"\nfocus_model = \"click\"\n\n[keybinds]\nq = \"quit\"\nw = \"kill\"\n";
    let response_1: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(good_toml);
    assert!(response_1.is_ok());
    let a_config = response_1.unwrap();
    let response_2: std::result::Result<String, toml::ser::Error> = toml::to_string(&a_config);
    assert!(response_2.is_ok());
    let maybe_toml = response_2.unwrap();
    assert_eq!(
        maybe_toml == good_toml || maybe_toml == alternate_toml,
        true
    );
}

/// Verify that deserializing into a Config object will fail on bad input.
#[test]
fn check_deserialize_errors() {
    // Cannot test the full range of deserialization errors, as during testing an X11 server may
    // not be available. An X11 server is required for `translate_keybinds` to map Keysyms to
    // Keycodes when populating `Config.keybinds`.
    let bad_mask_toml =
        "startup = [\"xterm\", \"xclock\"]\nmod_mask = \"modulo4\"\nfocus_model = \"click\"\n\n[keybinds]\nw = \"kill\"\nq = \"quit\"\n";
    let response_1: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(bad_mask_toml);
    assert!(response_1.is_err());

    let bad_focus_model_toml =
        "startup = [\"xterm\", \"xclock\"]\nmod_mask = \"mod4\"\nfocus_model = \"let the cat decide\"\n\n[keybinds]\nw = \"kill\"\nq = \"quit\"\n";
    let response_2: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(bad_focus_model_toml);
    assert!(response_2.is_err());

    // While `ModMask::Any` exists to permit conversions between ModMask and xproto::ModMask; we don't want to permit
    // users to specify this value in Config.toml. Ensure it is rejected.
    let any_mask_toml =
        "startup = [\"xterm\", \"xclock\"]\nmod_mask = \"any\"\nfocus_model = \"click\"\n\n[keybinds]\nw = \"kill\"\nq = \"quit\"\n";
    let response_3: std::result::Result<
        Config<x11rb::rust_connection::RustConnection>,
        toml::de::Error,
    > = toml::from_str(any_mask_toml);
    assert!(response_3.is_err());
}
