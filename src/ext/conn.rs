use x11rb::connection::Connection;
use x11rb::cookie::Cookie;
use x11rb::errors::ConnectionError;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;

/// Trait that extends an X11 connection with some convenience functions.
pub(crate) trait OxConnectionExt: Connection {
    /// Like `get_property`, but with fewer parameters, so the caller has fewer
    /// things to worry about.
    fn get_property_simple<A, B>(
        &self,
        window: xproto::Window,
        property: A,
        type_: B,
    ) -> std::result::Result<Cookie<'_, Self, xproto::GetPropertyReply>, ConnectionError>
    where
        A: Into<xproto::Atom>,
        B: Into<xproto::Atom>;
}

impl<Conn> OxConnectionExt for Conn
where
    Conn: Connection,
{
    fn get_property_simple<A, B>(
        &self,
        window: xproto::Window,
        property: A,
        type_: B,
    ) -> std::result::Result<
        x11rb::cookie::Cookie<'_, Conn, xproto::GetPropertyReply>,
        ConnectionError,
    >
    where
        Conn: Connection,
        A: Into<xproto::Atom>,
        B: Into<xproto::Atom>,
    {
        self.get_property(
            // Don't delete the property. (Why is this even an option in get_property()?)
            false, window, property, type_, 0, // Offset of 0.
            // Request an arbitrarily large number of words. (This is
            // approximately one million words, which is what XGetWMProtocols uses.)
            0x100000,
        )
    }
}
