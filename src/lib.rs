use serde::Deserialize;
use serde::Serialize;

use structopt::StructOpt;

use std::collections::HashMap;
use std::error::Error;

use x11rb::protocol::xproto;

/// We always use this type for errors, except where the type system forces us
/// to use something else.
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Local data about a top-level window.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Client {
    /// Horizontal position.
    pub x: i16,
    /// Vertical position.
    pub y: i16,
    /// Horizontal extent.
    pub width: u16,
    /// Vertical extent.
    pub height: u16,
}

/// The state of the window manager.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OxWMState {
    pub clients: HashMap<xproto::Window, Client>,
}

#[derive(Clone, Debug, Serialize, Deserialize, StructOpt)]
#[structopt(about = "control OxWM")]
pub enum Request {
    Ls,
}
