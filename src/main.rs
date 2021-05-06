use log;
use std::error::Error;
use std::process::Command;
use std::result::Result;
use x11rb::protocol::xproto;
use x11rb::protocol::Event::*;
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::xproto::ConnectionExt,
};

const TERM: &str = "xterm";
const MOD: xproto::ModMask = xproto::ModMask::M4;

struct OxWM<Conn> {
    conn: Conn,
    screen: xproto::Screen,
    clients: Clients,
    keep_going: bool,
}

impl<Conn> OxWM<Conn> {
    fn new(conn: Conn, screen: usize) -> Result<OxWM<Conn>, Box<dyn Error>>
    where
        Conn: Connection,
    {
        // Unfortunately, we can't acquire a connection here; we have to accept
        // one as an argument. Why? Because `x11rb::connect` returns an
        // existential `Connection`, but `Conn` is universally quantified. In
        // order to acquire a connection here, we'd need to be able to do
        // something like `impl OxWM<typeof(x11rb::connect().1)> {...}`, which
        // isn't even close to valid Rust.
        let setup = conn.setup();
        let screen = setup.roots[screen].clone();
        let root = screen.root;
        let mut ret = OxWM {
            conn: conn,
            screen: screen,
            clients: Clients::new(),
            keep_going: true,
        };

        // Try to redirect structure events from children of the root window.
        // Only one client---which must be the WM, essentially by
        // definition---can do this; so if we fail here, another WM is probably
        // running.
        log::debug!("Selecting SUBSTRUCTURE_REDIRECT on the root window.");
        xproto::change_window_attributes(
            &ret.conn,
            root,
            &xproto::ChangeWindowAttributesAux::new()
                .event_mask(xproto::EventMask::SUBSTRUCTURE_REDIRECT),
        )?
        .check()?;
        // Start a terminal.
        if let Err(err) = Command::new(TERM).spawn() {
            log::error!("Unable to start terminal: {:}", err);
        }
        // Adopt already-existing windows.
        ret.adopt_children(root)?;
        // Initialize the keysym mapping.
        let keysyms =
            xproto::get_keyboard_mapping(&ret.conn, xproto::Keycode::MIN, u8::MAX)?.reply()?;
        // Get a passive grab on Super+keycode 24 (happens to be Q on my
        // keybard; don't worry, customizable keybinds are the next goal).
        xproto::grab_key(
            &ret.conn,
            false,
            root,
            MOD,
            24,
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
        )?
        .check()?;
        Ok(ret)
    }

    /// Run the WM. Note that this consumes the OxWM object: in particular, once
    /// this procedure returns, the connection to the X server is gone.
    fn run(mut self) -> Result<(), Box<dyn Error>>
    where
        Conn: Connection,
    {
        // Core event loop.
        while self.keep_going {
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
                KeyPress(ev) => {
                    break;
                }
                MapRequest(ev) => {
                    xproto::map_window(&self.conn, ev.window)?.check()?;
                }
                MappingNotify(_) => (),
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
    OxWM::new(conn, screen)?.run()
}

fn main() -> Result<(), Box<dyn Error>> {
    simple_logger::SimpleLogger::new().init()?;
    run_wm()
}
