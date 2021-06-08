use thiserror::Error;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt as _;

use crate::Result;

/// A client's WM_PROTOCOLS. We ignore the deprecated WM_SAVE_YOURSELF protocol.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub struct WmProtocols {
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

/// A client's WM_STATE.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug)]
pub enum WmState {
    /// The Withdrawn state.
    Withdrawn,
    /// The Normal state.
    Normal,
    /// The Iconic state.
    Iconic,
}

impl From<WmState> for u32 {
    fn from(st: WmState) -> Self {
        match st {
            WmState::Withdrawn => 0,
            WmState::Normal => 1,
            WmState::Iconic => 3,
        }
    }
}

/// An error indicating that a client's property had an unrecoverable unexpected
/// format or value.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug, Error)]
#[error("error while decoding atom")]
struct AtomDecodeError;

/// Keeps track of standard ICCCM atoms, and provides a few functions for
/// getting/setting certain properties.
pub struct Atoms {
    /// The interned WM_DELETE_WINDOW atom.
    pub(crate) wm_delete_window: xproto::Atom,
    /// The interned WM_PROTOCOLS atom.
    pub(crate) wm_protocols: xproto::Atom,
    /// The interned WM_SAVE_YOURSELF atom.
    pub(crate) wm_save_yourself: xproto::Atom,
    /// The interned WM_TAKE_FOCUS atom.
    pub(crate) wm_take_focus: xproto::Atom,
}

impl Atoms {
    /// Create a new `Atoms` object by interning some atoms.
    pub fn new<Conn>(conn: &Conn) -> Result<Atoms>
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
            wm_take_focus,
        })
    }

    /// Send a WM_DELETE_WINDOW message.
    pub fn delete_window<Conn>(&self, conn: &Conn, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        let mut data = [0; 5];
        data[0] = self.wm_delete_window;
        data[1] = x11rb::CURRENT_TIME;
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

    /// Get a WM_PROTOCOLS property.
    pub fn get_wm_protocols<Conn>(
        &self,
        conn: &Conn,
        window: xproto::Window,
    ) -> Result<Option<WmProtocols>>
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
        if reply.format == 0 {
            return Ok(None);
        }
        let reply = reply.value32().ok_or(AtomDecodeError)?;
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
        Ok(Some(ret))
    }
}
