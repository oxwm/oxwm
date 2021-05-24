use essrpc::essrpc;
use essrpc::RPCError;

use serde::Deserialize;
use serde::Serialize;

use std::collections::HashMap;
use std::error::Error;

use x11rb::protocol::xproto;

/// Our most common error type. Policy right now is to use this unless there
/// is a specific reason to use something else.
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Local data about a top-level window.
#[derive(Debug, Deserialize, Serialize)]
pub struct Client {
    /// Horizontal position.
    pub x: i16,
    /// Vertical position.
    pub y: i16,
    /// Horizontal extent.
    pub width: u16,
    /// Vertical extent.
    pub height: u16,
    /// Name. Right now, this is only obtained from the `WM_NAME` property. In
    /// the future, it may also be obtained from `_NET_WM_NAME`.
    pub name: String,
}

#[essrpc]
pub trait OxWM {
    // essrpc requires that the error type be convertible to RPCError
    fn ls(&self) -> std::result::Result<HashMap<xproto::Window, Client>, RPCError>;
}
