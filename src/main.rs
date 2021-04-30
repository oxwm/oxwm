use log;
use std::error::Error;
use std::result::Result;
use x11rb::protocol::xproto;
use x11rb::protocol::Event::*;
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::xproto::ConnectionExt,
};

struct OxWM<Conn> {
    conn: Conn,
    screen: usize,
    clients: Clients,
}

impl<Conn> OxWM<Conn> {
    fn new(conn: Conn, screen: usize) -> OxWM<Conn> {
        // Unfortunately, we can't acquire a connection here; we have to accept
        // one as an argument. Why? Because `x11rb::connect` returns an
        // existential `Connection`, but `Conn` is universally quantified. In
        // order to acquire a connection here, we'd need to be able to do
        // something like `impl OxWM<typeof(x11rb::connect().1)> {...}`, which
        // isn't even close to valid Rust.
        OxWM {
            conn: conn,
            screen: screen,
            clients: Clients::new(),
        }
    }

    fn run(&mut self) -> Result<(), Box<dyn Error>>
    where
        Conn: Connection,
    {
        let setup = self.conn.setup();
        let screen = &setup.roots[self.screen];
        let root = screen.root;
        // Try to redirect structure events from children of the root window.
        // Only one client---which must be the WM, essentially by
        // definition---can do this; so if we fail here, another WM is probably
        // running.
        log::debug!("Selecting SUBSTRUCTURE_REDIRECT on the root window.");
        xproto::change_window_attributes(
            &self.conn,
            root,
            &xproto::ChangeWindowAttributesAux::new()
                .event_mask(xproto::EventMask::SUBSTRUCTURE_REDIRECT),
        )?
        .check()?;
        // Adopt already-existing windows.
        self.adopt_children(root)?;
        // Core event loop.
        loop {
            let ev = self.conn.wait_for_event()?;
            log::debug!("{:?}", ev);
            match ev {
                ConfigureRequest(ev) => {
                    self.conn
                        .configure_window(
                            ev.window,
                            &xproto::ConfigureWindowAux::from_configure_request(&ev),
                        )?
                        .check()?;
                }
                MapRequest(ev) => {
                    xproto::map_window(&self.conn, ev.window)?.check()?;
                }
                _ => {
                    log::warn!("Unhandled event!");
                }
            }
        }
        Ok(())
    }

    /// Adopt all children of the given window.
    fn adopt_children(&mut self, root: xproto::Window) -> Result<(), Box<dyn Error>>
    where
        Conn: Sized + RequestConnection,
    {
        let children = xproto::query_tree(&self.conn, root)?.reply()?.children;
        self.adopt_windows(children.into_iter())?;
        Ok(())
    }

    /// Adopt every window in the provided iterator.
    fn adopt_windows<Iter>(&mut self, windows: Iter) -> Result<(), Box<dyn Error>>
    where
        Conn: RequestConnection,
        Iter: Iterator<Item = xproto::Window>,
    {
        let conn = &self.conn;
        // Request information about all the children of the root window. We
        // send out all the requests before listening for any replies.
        let cookies = windows
            .map(|window| {
                (
                    window,
                    xproto::get_window_attributes(conn, window),
                    xproto::get_geometry(conn, window),
                )
            })
            .collect::<Vec<_>>();
        for (window, cookie1, cookie2) in cookies {
            // If the cookie is an Err, then there was a connection error, which
            // is fatal. But if the reply is an Err, that just means that the
            // window is gone.
            if let (Ok(attrs), Ok(geom)) = (cookie1?.reply(), cookie2?.reply()) {
                self.clients.add_window(window, attrs, geom)
            }
        }
        Ok(())
    }
}

struct Client {
    window: xproto::Window,
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

struct Clients {
    clients: Vec<Client>,
}

impl Clients {
    fn new() -> Clients {
        Clients {
            clients: Vec::new(),
        }
    }

    fn add_window(
        &mut self,
        window: xproto::Window,
        attrs: xproto::GetWindowAttributesReply,
        geom: xproto::GetGeometryReply,
    ) {
        self.clients.push(Client {
            window: window,
            x: geom.x,
            y: geom.y,
            width: geom.width,
            height: geom.height,
        })
    }
}

fn run_wm() -> Result<(), Box<dyn Error>> {
    let (conn, screen) = x11rb::connect(None)?;
    log::info!("Connected on screen {}.", screen);
    OxWM::new(conn, screen).run()
}

fn main() -> Result<(), Box<dyn Error>> {
    simple_logger::SimpleLogger::new().init()?;
    run_wm()
}
