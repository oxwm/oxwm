use x11rb::connection::Connection;
use x11rb::connection::RequestConnection;
use x11rb::cookie::Cookie;
use x11rb::errors::ConnectionError;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;

/// Trait that extends an X11 connection with some convenience functions.
pub(crate) trait OxConnectionExt: RequestConnection {
    /// Like `get_property`, but with fewer parameters, so you have fewer things
    /// to worry about.
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
            false, window, property, type_, 0,
            // Arbitrary large number. (Approximately one million, which is
            // what XGetWMProtocols uses.)
            0x100000,
        )
    }
}
