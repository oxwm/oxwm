use std::collections::HashMap;
use std::error::Error;
use std::ops::Deref;
use std::str::FromStr;

use essrpc::essrpc;
use essrpc::RPCError;
use essrpc::RPCErrorKind;

use serde::Deserialize;
use serde::Serialize;

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
    pub name: Vec<u8>,
}

/// Local state of the window manager.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OxWMState {
    pub clients: HashMap<xproto::Window, Client>,
}

/// Bespoke `StackMode` type so that we can implement `Serialize` and
/// `Deserialize`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum StackMode {
    Above,
    Below,
    TopIf,
    BottomIf,
    Opposite,
}

impl From<StackMode> for xproto::StackMode {
    fn from(stack_mode: StackMode) -> Self {
        match stack_mode {
            StackMode::Above => xproto::StackMode::ABOVE,
            StackMode::Below => xproto::StackMode::BELOW,
            StackMode::TopIf => xproto::StackMode::TOP_IF,
            StackMode::BottomIf => xproto::StackMode::BOTTOM_IF,
            StackMode::Opposite => xproto::StackMode::OPPOSITE,
        }
    }
}

/// RPC protocol.
#[essrpc]
pub trait Ox {
    fn ls(&self) -> std::result::Result<OxWMState, RPCError>;
    fn configure_window(
        &self,
        window: xproto::Window,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        border_width: Option<u32>,
        sibling: Option<xproto::Window>,
        stack_mode: Option<StackMode>,
    ) -> std::result::Result<(), RPCError>;
}

impl<T, U> Ox for T
where
    T: Deref<Target = U>,
    U: Ox,
{
    fn ls(&self) -> std::result::Result<OxWMState, essrpc::RPCError> {
        self.deref().ls()
    }

    fn configure_window(
        &self,
        window: xproto::Window,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        border_width: Option<u32>,
        sibling: Option<xproto::Window>,
        stack_mode: Option<StackMode>,
    ) -> std::result::Result<(), RPCError> {
        self.deref().configure_window(
            window,
            x,
            y,
            width,
            height,
            border_width,
            sibling,
            stack_mode,
        )
    }
}

pub trait IntoRPCError<T> {
    fn into_rpc_error(self) -> std::result::Result<T, RPCError>;
}

impl<T, E> IntoRPCError<T> for std::result::Result<T, E>
where
    E: Error,
{
    fn into_rpc_error(self) -> std::result::Result<T, RPCError> {
        self.map_err(|err| RPCError::with_cause(RPCErrorKind::Other, "", err))
    }
}
