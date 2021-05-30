mod config;
mod ext;
mod util;

use std::collections::HashMap;
use std::error::Error;
use std::process::Command;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConfigureWindowAux;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::Event::*;

use config::*;
use ext::conn::OxConnectionExt;
use util::*;

/// Minimum client width.
const MIN_WIDTH: u32 = 256;
/// Minimum client height.
const MIN_HEIGHT: u32 = 256;

/// General-purpose result type. Not very precise, but we're not actually doing
/// anything with errors other than letting them bubble up to the user, so this
/// is fine for now.
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) struct OxWM<Conn> {
    /// The source of all our problems.
    conn: Conn,
    /// The index of the screen we're connected on.
    screen: usize,
    /// Configuration data.
    config: Config<Conn>,
    /// Map of frame windows to local client data.
    clients: HashMap<xproto::Window, Client>,
    /// "Keep going" flag. If this is set to `false` at the start of the event
    /// loop, the window manager will stop running.
    keep_going: bool,
    /// If a window is being dragged, then that state is stored here.
    drag: Option<Drag>,
}

impl<Conn> OxWM<Conn> {
    /// Initialize the window manager.
    fn new(conn: Conn, screen: usize) -> Result<OxWM<Conn>>
    where
        Conn: Connection,
    {
        // Unfortunately, we can't acquire a connection here; we have to accept
        // one as an argument. Why? Because `x11rb::connect` returns an
        // existential `Connection`, but `Conn` is universally quantified.
        log::debug!("Loading config file.");
        // Load the config file first, since this is where errors are most
        // likely to occur.
        let config = Config::load()?;
        let mut ret = OxWM {
            conn,
            screen,
            config,
            clients: HashMap::new(),
            keep_going: true,
            drag: None,
        };
        // Grab the server so that we can do setup atomically. We don't need to
        // worry about ungrabbing if we fail: this function consumes the
        // connection, so if we fail, the connection will just get dropped.
        //
        // TODO Not sure whether it's strictly necessary to grab the server, but
        // it gives me some peace of mind. Should probably investigate.
        ret.conn.grab_server()?.check()?;
        ret.init()?;
        ret.conn.ungrab_server()?.check()?;
        Ok(ret)
    }

    /// Perform setup and initialization.
    fn init(&mut self) -> Result<()>
    where
        Conn: Connection,
    {
        // Try to become the window manager early, so that we can fail early
        // if necessary.
        self.become_wm()?;
        self.manage_extant_clients()?;
        self.global_setup()?;
        self.run_startup_programs()?;
        Ok(())
    }

    /// Try to become the window manager.
    fn become_wm(&self) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Trying to become the window manager.");
        self.conn
            .change_window_attributes(
                self.root(),
                &xproto::ChangeWindowAttributesAux::new()
                    .event_mask(xproto::EventMask::SUBSTRUCTURE_REDIRECT),
            )?
            .check()?;
        Ok(())
    }

    /// Find extant clients and manage them.
    fn manage_extant_clients(&mut self) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Managing extant clients.");
        let children = self.conn.query_tree(self.root())?.reply()?.children;
        for window in children {
            self.manage(window)?;
        }
        Ok(())
    }

    /// Perform global setup operations that involve the server.
    fn global_setup(&self) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Setting event mask on the root window.");
        self.conn
            .change_window_attributes(
                self.root(),
                &xproto::ChangeWindowAttributesAux::new().event_mask(
                    xproto::EventMask::SUBSTRUCTURE_NOTIFY
                        | xproto::EventMask::SUBSTRUCTURE_REDIRECT,
                ),
            )?
            .check()?;
        log::debug!("Grabbing bound keycodes.");
        self.config
            .keybinds
            .keys()
            .map(|keycode| {
                self.conn.grab_key(
                    false,
                    self.root(),
                    self.config.mod_mask,
                    *keycode,
                    xproto::GrabMode::ASYNC,
                    xproto::GrabMode::ASYNC,
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .try_for_each(|cookie| cookie?.check())?;
        Ok(())
    }

    /// Run configured startup programs.
    fn run_startup_programs(&self) -> Result<()> {
        log::debug!("Running startup programs.");
        for program in &self.config.startup {
            if let Err(err) = Command::new(program).spawn() {
                log::warn!("Unable to execute startup program `{}': {:?}", program, err);
            }
        }
        Ok(())
    }

    /// Run the WM. Note that this consumes the OxWM object: once
    /// this procedure returns, the connection to the X server is gone.
    fn run(mut self) -> Result<()>
    where
        Conn: Connection,
    {
        while self.keep_going {
            let ev = self.conn.wait_for_event()?;
            log::trace!("{:?}", ev);
            match ev {
                ButtonPress(ev) => {
                    let window = ev.event;
                    self.click(window)?;
                    if ev.state & u16::from(self.config.mod_mask) == 0 {
                        log::trace!("regular click");
                        self.conn
                            .allow_events(xproto::Allow::REPLAY_POINTER, x11rb::CURRENT_TIME)?
                            .check()?;
                    } else {
                        self.begin_drag(window, ev.detail, ev.event_x, ev.event_y);
                    }
                }
                ButtonRelease(_) => self.drag = None,
                ConfigureNotify(ev) => {
                    if let Some(client) = self.clients.get_mut(&ev.window) {
                        client.x = ev.x;
                        client.y = ev.y;
                        client.width = ev.width;
                        client.height = ev.height;
                    }
                }
                ConfigureRequest(ev) => {
                    let value_list = xproto::ConfigureWindowAux::from_configure_request(&ev);
                    let width = value_list.width.map(|w| w.max(MIN_WIDTH));
                    let height = value_list.height.map(|h| h.max(MIN_HEIGHT));
                    self.conn
                        .configure_window(ev.window, &value_list.width(width).height(height))?;
                }
                DestroyNotify(ev) => {
                    self.clients.remove(&ev.window);
                    // If we were dragging the window, stop dragging it.
                    if let Some(ref drag) = self.drag {
                        if drag.window == ev.window {
                            self.drag = None;
                        }
                    }
                }
                EnterNotify(ev) => {
                    let window = ev.event;
                    if let FocusModel::Autofocus | FocusModel::Autoraise = self.config.focus_model {
                        self.focus(window)?;
                    }
                    if let FocusModel::Autoraise = self.config.focus_model {
                        self.raise(window)?;
                    }
                }
                KeyPress(ev) => {
                    // We're only listening for keycodes that are bound in the keybinds
                    // map (anything else is a bug), so we can call unwrap() with a
                    // clean conscience here.
                    let action = self.config.keybinds.get(&ev.detail).unwrap();
                    action(&mut self, ev.event)?;
                }
                MapRequest(ev) => {
                    self.manage(ev.window)?;
                    self.conn.map_window(ev.window)?.check()?
                }
                MotionNotify(ev) => match self.drag {
                    None => log::error!("MotionNotify event without a drag."),
                    Some(ref drag) => {
                        let config = match drag.type_ {
                            DragType::MOVE => {
                                let x = (ev.root_x - drag.x) as i32;
                                let y = (ev.root_y - drag.y) as i32;
                                ConfigureWindowAux::new().x(x).y(y)
                            }
                            DragType::RESIZE(corner) => {
                                // The client must still be in the map, since the event
                                // we're processing now can't temporally succeed a
                                // DestroyWindow event for the same window.
                                let client = self.clients.get(&drag.window).unwrap();
                                match corner {
                                    Corner::LeftTop => {
                                        let mut x = ev.root_x - drag.x;
                                        let mut width =
                                            client.width as i32 - ((x - client.x) as i32);
                                        if width < MIN_WIDTH as i32 {
                                            width = MIN_WIDTH as i32;
                                            x = client.x;
                                        }
                                        let width = width as u32;
                                        let x = x as i32;
                                        let mut y = ev.root_y - drag.y;
                                        let mut height =
                                            client.height as i32 - ((y - client.y) as i32);
                                        if height < MIN_HEIGHT as i32 {
                                            height = MIN_HEIGHT as i32;
                                            y = client.y;
                                        }
                                        let height = height as u32;
                                        let y = y as i32;
                                        ConfigureWindowAux::new()
                                            .x(x)
                                            .y(y)
                                            .width(width)
                                            .height(height)
                                    }
                                    Corner::LeftBottom => {
                                        let height =
                                            ((ev.event_y - drag.y).max(0) as u32).max(MIN_HEIGHT);
                                        let mut x = ev.root_x - drag.x;
                                        let mut width =
                                            client.width as i32 - ((x - client.x) as i32);
                                        if width < MIN_WIDTH as i32 {
                                            width = MIN_WIDTH as i32;
                                            x = client.x;
                                        }
                                        let width = width as u32;
                                        let x = x as i32;
                                        ConfigureWindowAux::new().x(x).width(width).height(height)
                                    }
                                    Corner::RightTop => {
                                        let width =
                                            ((ev.event_x - drag.x).max(0) as u32).max(MIN_WIDTH);
                                        let mut y = ev.root_y - drag.y;
                                        let mut height =
                                            client.height as i32 - ((y - client.y) as i32);
                                        if height < MIN_HEIGHT as i32 {
                                            height = MIN_HEIGHT as i32;
                                            y = client.y;
                                        }
                                        let height = height as u32;
                                        let y = y as i32;
                                        ConfigureWindowAux::new().y(y).width(width).height(height)
                                    }
                                    Corner::RightBottom => {
                                        let width =
                                            ((ev.event_x - drag.x).max(0) as u32).max(MIN_WIDTH);
                                        let height =
                                            ((ev.event_y - drag.y).max(0) as u32).max(MIN_WIDTH);
                                        ConfigureWindowAux::new().width(width).height(height)
                                    }
                                }
                            }
                        };
                        self.conn.configure_window(drag.window, &config)?.check()?;
                    }
                },
                _ => log::warn!("Unhandled event!"),
            }
        }
        Ok(())
    }

    /// A button has been clicked (without the modifier).
    fn click(&self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        self.focus(window)?;
        self.raise(window)?;
        Ok(())
    }

    /// Focus a window.
    fn focus(&self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        self.conn
            .set_input_focus(
                xproto::InputFocus::POINTER_ROOT,
                window,
                x11rb::CURRENT_TIME,
            )?
            .check()?;
        Ok(())
    }

    /// Raise a window to the front of the stack.
    fn raise(&self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        self.conn
            .configure_window(
                window,
                &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
            )?
            .check()?;
        Ok(())
    }

    fn begin_drag(&mut self, window: xproto::Window, button: xproto::Button, x: i16, y: i16) {
        let client = self.clients.get(&window).unwrap();
        let (type_, corner) = match button {
            1 => (DragType::MOVE, Corner::LeftTop),
            3 => {
                // We resize from whatever corner the pointer is
                // closest to.
                let mid_x = (client.width / 2) as i16;
                let mid_y = (client.height / 2) as i16;
                let corner = match (x >= mid_x, y >= mid_y) {
                    (false, false) => Corner::LeftTop,
                    (false, true) => Corner::LeftBottom,
                    (true, false) => Corner::RightTop,
                    (true, true) => Corner::RightBottom,
                };
                (DragType::RESIZE(corner), corner)
            }
            _ => {
                log::error!("Invalid button.");
                return;
            }
        };
        let (cx, cy) = client.corner_rel(corner);
        let x = x - (cx as i16);
        let y = y - (cy as i16);
        self.drag = Some(Drag {
            type_,
            window,
            x,
            y,
        });
    }

    fn manage(&mut self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        let cookie1 = self.conn.get_window_attributes(window);
        let cookie2 = self.conn.get_geometry(window);
        // Get WM_NAME.
        let cookie3 = self.conn.get_property_simple(
            window,
            xproto::AtomEnum::WM_NAME,
            xproto::AtomEnum::STRING,
        );
        // Grab modifier + nothing.
        let nomod: u16 = 0;
        // TODO I don't fully understand sync/async grab modes.
        let cookie4 = self.conn.grab_button(
            true,
            window,
            event_mask_to_u16(xproto::EventMask::BUTTON_PRESS),
            xproto::GrabMode::SYNC,
            xproto::GrabMode::SYNC,
            x11rb::NONE,
            x11rb::NONE,
            xproto::ButtonIndex::M1,
            nomod,
        );
        // Grab modifier + left mouse button.
        let cookie5 = self.conn.grab_button(
            false,
            window,
            event_mask_to_u16(
                xproto::EventMask::BUTTON_PRESS
                    | xproto::EventMask::BUTTON_RELEASE
                    | xproto::EventMask::POINTER_MOTION,
            ),
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            xproto::ButtonIndex::M1,
            self.config.mod_mask,
        );
        // Grab modifier + right mouse button.
        let cookie6 = self.conn.grab_button(
            false,
            window,
            event_mask_to_u16(
                xproto::EventMask::BUTTON_PRESS
                    | xproto::EventMask::BUTTON_RELEASE
                    | xproto::EventMask::POINTER_MOTION,
            ),
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            xproto::ButtonIndex::M3,
            self.config.mod_mask,
        );
        // Set our desired event mask.
        let cookie7 = self.conn.change_window_attributes(
            window,
            &xproto::ChangeWindowAttributesAux::new()
                .event_mask(xproto::EventMask::ENTER_WINDOW | xproto::EventMask::PROPERTY_CHANGE),
        );
        // ...and put everything together at the end.]
        match (
            cookie1?.reply(),
            cookie2?.reply(),
            cookie3?.reply(),
            cookie4?.check(),
            cookie5?.check(),
            cookie6?.check(),
            cookie7?.check(),
        ) {
            (Ok(attrs), Ok(geom), Ok(prop_wm_name), Ok(_), Ok(_), Ok(_), Ok(_)) => {
                if !attrs.override_redirect {
                    // TODO Implement compound text decoding.
                    let name = String::from_utf8(prop_wm_name.value).unwrap();
                    log::debug!("Window name: {}.", name);
                    self.clients.insert(
                        window,
                        Client {
                            frame: 0,
                            contents: 0,
                            x: geom.x,
                            y: geom.y,
                            width: geom.width,
                            height: geom.height,
                            name,
                        },
                    );
                }
            }
            err => log::warn!(
                "Error while trying to manage the window (one of these): {:?}",
                err
            ),
        }
        Ok(())
    }

    // Actions go here. Note that, due to the need to conform to the Action
    // type, these functions' type signatures may sometimes seem odd.

    /// Kill the currently-focused client.
    fn kill_focused_client(&mut self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        self.conn.kill_client(window)?.check()?;
        Ok(())
    }

    /// Poison the window manager, causing it to die promptly.
    fn poison(&mut self, _: xproto::Window) -> Result<()> {
        self.keep_going = false;
        Ok(())
    }

    // Simple utility stuff goes here.

    /// Get the root window.
    fn root(&self) -> xproto::Window
    where
        Conn: Connection,
    {
        self.conn.setup().roots[self.screen].root
    }
}

#[derive(Clone, Debug)]
struct Client {
    /// The frame window.
    frame: xproto::Window,
    /// The "real" client window.
    contents: xproto::Window,
    /// Horizontal position.
    x: i16,
    /// Vertical position.
    y: i16,
    /// Horizontal extent.
    width: u16,
    /// Vertical extent.
    height: u16,
    /// Client name.
    name: String,
}

impl Client {
    fn corner_rel(&self, corner: Corner) -> (u16, u16) {
        match corner {
            Corner::LeftTop => (0, 0),
            Corner::LeftBottom => (0, self.height),
            Corner::RightTop => (self.width, 0),
            Corner::RightBottom => (self.width, self.height),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Corner {
    LeftTop,
    LeftBottom,
    RightTop,
    RightBottom,
}

#[derive(Clone, Debug)]
enum DragType {
    MOVE,
    RESIZE(Corner),
}

/// The state of a window drag.
#[derive(Clone, Debug)]
struct Drag {
    /// The type of drag.
    type_: DragType,
    /// The window that is being dragged.
    window: xproto::Window,
    /// The x-position of the pointer relative to (a certain corner of) the
    /// window.
    x: i16,
    /// The x-position of the pointer relative to (a certain corner of) the
    /// window.
    y: i16,
}

fn run_wm() -> Result<()> {
    log::debug!("Connecting to the X server.");
    let (conn, screen) = x11rb::connect(None)?;
    log::info!("Connected on screen {}.", screen);
    log::debug!("Initializing OxWM.");
    let oxwm = OxWM::new(conn, screen)?;
    log::debug!("Running OxWM.");
    oxwm.run()
}

fn main() -> Result<()> {
    simple_logger::SimpleLogger::new().init()?;
    run_wm()
}
