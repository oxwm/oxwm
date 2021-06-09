//! Atom management and mid-level routines for getting/setting properties.

use std::convert::TryFrom;

use x11rb::connection::Connection;
use x11rb::errors::ConnectionError;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::ReplyError;
use x11rb::wrapper::ConnectionExt as _;

use crate::Result;

/// A client's WM_PROTOCOLS. We ignore the deprecated WM_SAVE_YOURSELF protocol.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub(crate) struct WmProtocols {
    /// Whether the client supports WM_TAKE_FOCUS.
    pub take_focus: bool,
    /// Whether the client supports WM_DELETE_WINDOW.
    pub delete_window: bool,
}

impl WmProtocols {
    /// Default value for WM_PROTOCOLS, indicating no supported protocols.
    pub(crate) fn new() -> WmProtocols {
        WmProtocols {
            take_focus: false,
            delete_window: false,
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug)]
pub(crate) struct WmState {
    /// The WM_STATE.state field, indicating the state that the window is in.
    pub(crate) state: WmStateState,
    /// The WM_STATE.icon field, indicating the icon that represents the client.
    pub(crate) icon: xproto::Window,
}

impl From<WmState> for [u32; 2] {
    fn from(value: WmState) -> [u32; 2] {
        [u32::from(value.state), value.icon]
    }
}

impl TryFrom<&[u32]> for WmState {
    type Error = ();

    fn try_from(value: &[u32]) -> std::result::Result<Self, Self::Error> {
        match value[..] {
            [state, icon] => Ok(WmState {
                state: WmStateState::try_from(state)?,
                icon,
            }),
            _ => Err(()),
        }
    }
}

/// Possible values for WM_STATE.state.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug)]
pub(crate) enum WmStateState {
    /// The Withdrawn state.
    Withdrawn,
    /// The Normal state.
    Normal,
    /// The Iconic state.
    Iconic,
}

impl From<WmStateState> for u32 {
    fn from(value: WmStateState) -> Self {
        match value {
            WmStateState::Withdrawn => 0,
            WmStateState::Normal => 1,
            WmStateState::Iconic => 3,
        }
    }
}

impl TryFrom<u32> for WmStateState {
    type Error = ();

    fn try_from(value: u32) -> std::result::Result<Self, Self::Error> {
        match value {
            0 => Ok(WmStateState::Withdrawn),
            1 => Ok(WmStateState::Normal),
            3 => Ok(WmStateState::Iconic),
            _ => Err(()),
        }
    }
}

/// Keeps track of standard ICCCM atoms, and provides a few functions for
/// getting/setting certain properties.
pub(crate) struct Atoms {
    /// The interned WM_DELETE_WINDOW atom.
    pub(crate) wm_delete_window: xproto::Atom,
    /// The interned WM_PROTOCOLS atom.
    pub(crate) wm_protocols: xproto::Atom,
    /// The interned WM_SAVE_YOURSELF atom.
    pub(crate) wm_save_yourself: xproto::Atom,
    /// The interned WM_STATE atom.
    pub(crate) wm_state: xproto::Atom,
    /// The interned WM_TAKE_FOCUS atom.
    pub(crate) wm_take_focus: xproto::Atom,
}

impl Atoms {
    /// Create a new `Atoms` object by interning some atoms.
    pub(crate) fn new<Conn>(conn: &Conn) -> Result<Atoms>
    where
        Conn: Connection,
    {
        log::trace!("Interning WM_DELETE_WINDOW.");
        let wm_delete_window = conn
            .intern_atom(false, "WM_DELETE_WINDOW".as_bytes())?
            .reply()?
            .atom;
        log::trace!("Interning WM_PROTOCOLS.");
        let wm_protocols = conn
            .intern_atom(false, "WM_PROTOCOLS".as_bytes())?
            .reply()?
            .atom;
        log::trace!("Interning WM_SAVE_YOURSELF.");
        let wm_save_yourself = conn
            .intern_atom(false, "WM_SAVE_YOURSELF".as_bytes())?
            .reply()?
            .atom;
        log::trace!("Interning WM_STATE.");
        let wm_state = conn
            .intern_atom(false, "WM_STATE".as_bytes())?
            .reply()?
            .atom;
        log::trace!("Interning WM_TAKE_FOCUS.");
        let wm_take_focus = conn
            .intern_atom(false, "WM_TAKE_FOCUS".as_bytes())?
            .reply()?
            .atom;
        log::trace!("All atoms successfully interned.");
        Ok(Atoms {
            wm_delete_window,
            wm_protocols,
            wm_save_yourself,
            wm_state,
            wm_take_focus,
        })
    }

    /// Send a WM_DELETE_WINDOW message.
    pub(crate) fn delete_window<Conn>(&self, conn: &Conn, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        let data = [self.wm_delete_window, 0, 0, 0, 0];
        conn.send_event(
            false,
            window,
            xproto::EventMask::NO_EVENT,
            xproto::ClientMessageEvent {
                response_type: xproto::CLIENT_MESSAGE_EVENT,
                format: 32,
                sequence: 0,
                window,
                type_: self.wm_protocols,
                data: xproto::ClientMessageData::from(data),
            },
        )?
        .check()?;
        Ok(())
    }

    /// Get a window's WM_PROTOCOLS property. If the property is not set, a default value is used.
    pub(crate) fn get_wm_protocols<Conn>(
        &self,
        conn: &Conn,
        window: xproto::Window,
    ) -> Result<WmProtocols>
    where
        Conn: Connection,
    {
        log::trace!("Reading WM_PROTOCOLS on window {}.", window);
        let reply = conn
            .get_property(
                false,
                window,
                self.wm_protocols,
                xproto::AtomEnum::ATOM,
                0,
                // Arbitrary length taken from XGetWmProtocols.
                1_000_000,
            )?
            .reply()?;
        log::trace!("Got reply: {:?}", reply);
        let reply = match reply.value32() {
            None => return Ok(WmProtocols::new()),
            Some(x) => x,
        };
        let mut ret = WmProtocols {
            take_focus: false,
            delete_window: false,
        };
        for atom in reply {
            if atom == self.wm_take_focus {
                ret.take_focus = true;
            } else if atom == self.wm_save_yourself {
                log::warn!("Ignoring deprecated WM_SAVE_YOURSELF.");
            } else if atom == self.wm_delete_window {
                ret.delete_window = true;
            } else {
                log::warn!("Ignoring unrecognized WM_PROTOCOL {}.", atom);
            }
        }
        Ok(ret)
    }

    /// Get a window's WM_NORMAL_HINTS property
    pub(crate) fn get_wm_normal_hints<Conn>(
        &self,
        conn: &Conn,
        window: xproto::Window,
    ) -> Result<WmSizeHints>
    where
        Conn: Connection,
    {
        match WmSizeHints::get(conn, window, xproto::AtomEnum::WM_NORMAL_HINTS)?.reply() {
            Ok(x) => Ok(x),
            Err(ReplyError::ConnectionError(ConnectionError::ParseError(_))) => {
                Ok(WmSizeHints::new())
            }
            Err(err) => Err(Box::new(err)),
        }
    }

    /// Get a window's WM_STATE property.
    pub(crate) fn get_wm_state<Conn>(
        &self,
        conn: &Conn,
        window: xproto::Window,
    ) -> Result<Option<WmState>>
    where
        Conn: Connection,
    {
        let reply = conn
            .get_property(false, window, self.wm_state, self.wm_state, 0, 2)?
            .reply()?;
        let reply = match reply.value32() {
            None => return Ok(None),
            Some(x) => x,
        }
        .collect::<Vec<_>>();
        Ok(match WmState::try_from(&reply[..]) {
            Ok(x) => Some(x),
            Err(_) => None,
        })
    }

    /// Set a window's WM_STATE property.
    pub(crate) fn set_wm_state<Conn>(
        &self,
        conn: &Conn,
        window: xproto::Window,
        state: WmState,
    ) -> Result<()>
    where
        Conn: Connection,
    {
        let state: [u32; 2] = state.into();
        conn.change_property32(
            xproto::PropMode::REPLACE,
            window,
            self.wm_state,
            self.wm_state,
            &state,
        )?
        .check()?;
        Ok(())
    }
}
