//! The top-level window manager object.

mod atom;
mod client;
mod config;
mod util;

use std::error::Error;
use std::process::Command;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConfigureWindowAux;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::Event::*;

use atom::*;
use client::*;
use config::*;
use util::*;

/// General-purpose result type. Not very precise, but we're not actually doing
/// much with errors other than letting them bubble up to the user, so this is
/// fine for now.
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Default minimum client width.
const MIN_WIDTH: u16 = 128;
/// Default maximum client width.
const MAX_WIDTH: u16 = 16384;
/// Minimum client height.
const MIN_HEIGHT: u16 = 128;
/// Default maximum client width.
const MAX_HEIGHT: u16 = 16384;

pub(crate) struct OxWM<Conn> {
    /// The source of all our problems.
    conn: Conn,
    /// The index of the screen we're connected on.
    screen: usize,
    /// Configuration data.
    config: Config<Conn>,
    /// Local client data.
    clients: Clients,
    /// "Keep going" flag. If this is set to `false` at the start of the event
    /// loop, the window manager will stop running.
    keep_going: bool,
    /// If a window is being dragged, then that state is stored here.
    drag: Option<Drag>,
    /// Manager for atoms that we need to intern.
    atoms: Atoms,
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
        //
        // (Well, that's probably not true right now, but IN THEORY...)
        let config = Config::load().or_else(|err| -> Result<Config<Conn>> {
            //File access errors
            if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
                match io_error.kind() {
                    std::io::ErrorKind::NotFound => log::info!("Configuration file not found."),
                    std::io::ErrorKind::PermissionDenied => {
                        log::error!(
                            "Permission denied trying to read configuration file, aborting"
                        );
                        return Err(err);
                    }
                    _ => return Err(err),
                }
            }
            // Deserialization format errors
            if let Some(de_error) = err.downcast_ref::<toml::de::Error>() {
                log::error!("Failed to parse config.toml: {}", de_error);
                return Err(err);
            }
            // Config.toml content errors
            if let Some(config_error) = err.downcast_ref::<ConfigError>() {
                log::error!("{}", config_error);
                return Err(err);
            };
            log::info!("Applying default configuration.");
            let default_config = Config::new().unwrap();
            default_config.save().map_err(|save_err| {
                log::error!("{}", save_err);
                save_err
            })?;
            Ok(default_config)
        })?;
        // Grab the server so that we can do setup atomically. We don't need to
        // worry about ungrabbing if we fail: this function consumes the
        // connection, so if we fail, the connection will just get dropped.
        //
        // TODO Not sure whether it's strictly necessary to grab the server, but
        // it gives me some peace of mind. Should probably investigate.
        conn.grab_server()?.check()?;
        log::debug!("Interning needed atoms.");
        let atoms = Atoms::new(&conn)?;
        let clients = Clients::new(&conn, screen, &atoms)?;
        let mut ret = OxWM {
            conn,
            screen,
            config,
            clients,
            keep_going: true,
            drag: None,
            atoms,
        };
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
        for client in self.clients.iter() {
            self.manage(&client)?;
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
                        self.conn
                            .allow_events(xproto::Allow::REPLAY_POINTER, x11rb::CURRENT_TIME)?
                            .check()?;
                    } else {
                        self.begin_drag(window, ev.detail, ev.event_x, ev.event_y);
                    }
                }
                ButtonRelease(_) => self.drag = None,
                ConfigureNotify(ev) => {
                    if ev.above_sibling == x11rb::NONE {
                        self.clients.move_to_bottom(ev.window);
                    } else {
                        self.clients.move_to_above(ev.window, ev.above_sibling);
                    }
                    if let Some(ref mut st) = self.clients.get_mut(ev.window).state {
                        st.x = ev.x;
                        st.y = ev.y;
                        st.width = ev.width;
                        st.height = ev.height;
                    }
                }
                ConfigureRequest(ev) => {
                    let st = self.clients.get(ev.window).state.as_ref().unwrap();
                    let (min_width, min_height) = st
                        .wm_normal_hints
                        .min_size
                        .unwrap_or((MIN_WIDTH as i32, MIN_HEIGHT as i32));
                    let (max_width, max_height) = st
                        .wm_normal_hints
                        .max_size
                        .unwrap_or((MAX_WIDTH as i32, MAX_HEIGHT as i32));
                    let mut value_list = xproto::ConfigureWindowAux::from_configure_request(&ev);
                    // Windows that have override-redirect set can do whatever they want.
                    if !self.clients.get(ev.window).override_redirect() {
                        value_list.width = value_list
                            .width
                            .map(|w| w.max(min_width as u32).min(max_width as u32));
                        value_list.height = value_list
                            .height
                            .map(|h| h.max(min_height as u32).min(max_height as u32));
                    }
                    if let Err(e) = self.conn.configure_window(ev.window, &value_list)?.check() {
                        // The window might have already been destroyed!
                        log::warn!("{:?}", e);
                    }
                }
                CreateNotify(ev) => match self.create_notify(ev) {
                    Ok(_) => (),
                    Err(err) => log::warn!("{:?}", err),
                },
                DestroyNotify(ev) => {
                    let window = ev.window;
                    if let Some(client) = self.clients.get_focus() {
                        if client.window == window {
                            // Focus the first managed client that we can find.
                            for client in self.clients.iter().rev().skip(1) {
                                if let Some(ref st) = client.state {
                                    if st.is_viewable {
                                        self.focus(client.window)?;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    // Have to check here in case the window got destroyed
                    // before we could add it.
                    if self.clients.has_client(window) {
                        self.clients.remove(window);
                    }
                    // If we were dragging the window, stop dragging it.
                    if let Some(ref drag) = self.drag {
                        if drag.window == window {
                            self.drag = None;
                        }
                    }
                }
                EnterNotify(ev) => {
                    let window = ev.event;
                    if let FocusModel::Autofocus = self.config.focus_model {
                        if let Err(err) = self.focus(window) {
                            log::warn!("{:?}", err);
                        }
                    }
                }
                FocusIn(ev) => {
                    self.clients.set_focus(ev.event);
                }
                FocusOut(_) => {
                    self.clients.set_focus(None);
                }
                KeyPress(ev) => {
                    let action = self.config.keybinds.get(&ev.detail).unwrap();
                    action(&mut self, ev.child)?;
                }
                MapNotify(ev) => {
                    let window = ev.window;
                    if let Some(ref mut st) = self.clients.get_mut(window).state {
                        st.is_viewable = true;
                    }
                    self.atoms.set_wm_state(
                        &self.conn,
                        window,
                        WmState {
                            state: WmStateState::Normal,
                            icon: x11rb::NONE,
                        },
                    )?;
                }
                MapRequest(ev) => self.conn.map_window(ev.window)?.check()?,
                MotionNotify(ev) => {
                    let st = self.clients.get(ev.event).state.as_ref().unwrap();
                    let (min_width, min_height) = st
                        .wm_normal_hints
                        .min_size
                        .unwrap_or((MIN_WIDTH as i32, MIN_HEIGHT as i32));
                    let (max_width, max_height) = st
                        .wm_normal_hints
                        .max_size
                        .unwrap_or((MAX_WIDTH as i32, MAX_HEIGHT as i32));
                    let drag = self.drag.as_ref().unwrap();
                    let mut config = match drag.type_ {
                        DragType::Move => {
                            let x = (ev.root_x - drag.x) as i32;
                            let y = (ev.root_y - drag.y) as i32;
                            ConfigureWindowAux::new().x(x).y(y)
                        }
                        DragType::Resize(corner) => match corner {
                            Corner::LeftTop => {
                                let mut x = ev.root_x - drag.x;
                                let mut width = st.width as i32 - ((x - st.x) as i32);
                                if width < min_width {
                                    width = min_width;
                                    x = ((st.x as i32) + (st.width as i32 - width)) as i16;
                                } else if width > max_width {
                                    width = max_width;
                                    x = ((st.x as i32) + (st.width as i32 - width)) as i16;
                                }
                                let width = width as u32;
                                let x = x as i32;
                                let mut y = ev.root_y - drag.y;
                                let mut height = st.height as i32 - ((y - st.y) as i32);
                                if height < min_height {
                                    height = min_height;
                                    y = ((st.y as i32) + (st.height as i32 - height)) as i16;
                                } else if height > max_height {
                                    height = max_height;
                                    y = ((st.y as i32) + (st.height as i32 - height)) as i16;
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
                                let height = ((ev.event_y - drag.y).max(0) as i32)
                                    .max(min_height)
                                    .min(max_height)
                                    as u32;
                                let mut x = ev.root_x - drag.x;
                                let mut width = st.width as i32 - ((x - st.x) as i32);
                                if width < min_width {
                                    width = min_width;
                                    x = ((st.x as i32) + (st.width as i32 - width)) as i16;
                                } else if width > max_width {
                                    width = max_width;
                                    x = ((st.x as i32) + (st.width as i32 - width)) as i16;
                                }
                                let width = width as u32;
                                let x = x as i32;
                                ConfigureWindowAux::new().x(x).width(width).height(height)
                            }
                            Corner::RightTop => {
                                let width = ((ev.event_x - drag.x).max(0) as i32)
                                    .max(min_width)
                                    .min(max_width)
                                    as u32;
                                let mut y = ev.root_y - drag.y;
                                let mut height = st.height as i32 - ((y - st.y) as i32);
                                if height < min_height {
                                    height = min_height;
                                    y = ((st.y as i32) + (st.height as i32 - height)) as i16;
                                } else if height > max_height {
                                    height = max_height;
                                    y = ((st.y as i32) + (st.height as i32 - height)) as i16;
                                }
                                let height = height as u32;
                                let y = y as i32;
                                ConfigureWindowAux::new().y(y).width(width).height(height)
                            }
                            Corner::RightBottom => {
                                let width = ((ev.event_x - drag.x).max(0) as i32)
                                    .max(min_width)
                                    .min(max_height)
                                    as u32;
                                let height = ((ev.event_y - drag.y).max(0) as i32)
                                    .max(min_height)
                                    .min(max_height)
                                    as u32;
                                ConfigureWindowAux::new().width(width).height(height)
                            }
                        },
                    };
                    if let (Some((base_width, base_height)), Some((width_inc, height_inc))) = (
                        st.wm_normal_hints.base_size,
                        st.wm_normal_hints.size_increment,
                    ) {
                        let base_width = base_width as u32;
                        let base_height = base_height as u32;
                        let width_inc = width_inc as u32;
                        let height_inc = height_inc as u32;
                        if let Some(width) = config.width {
                            let units = (width - base_width) / width_inc;
                            let pixels = units * width_inc;
                            config.width = Some(base_width + pixels);
                        }
                        if let Some(height) = config.height {
                            let units = (height - base_height) / height_inc;
                            let pixels = units * height_inc;
                            config.height = Some(base_height + pixels);
                        }
                    }
                    self.conn.configure_window(drag.window, &config)?.check()?;
                }
                PropertyNotify(ev) => {
                    if let Err(err) = self.property_notify(ev) {
                        log::warn!("{:?}", err);
                    }
                }
                UnmapNotify(ev) => {
                    let window = ev.window;
                    if let Some(client) = self.clients.get_focus() {
                        if client.window == window {
                            self.clients.set_focus(None);
                        }
                    }
                    if let Err(err) = self.atoms.set_wm_state(
                        &self.conn,
                        window,
                        WmState {
                            state: WmStateState::Withdrawn,
                            icon: x11rb::NONE,
                        },
                    ) {
                        log::warn!("{:?}", err);
                    }
                }
                _ => log::warn!("Unhandled event!"),
            }
        }
        Ok(())
    }

    /// Initiate a drag on the given window.
    fn begin_drag(&mut self, window: xproto::Window, button: xproto::Button, x: i16, y: i16) {
        let st = self.clients.get(window).state.as_ref().unwrap();
        let (type_, corner) = match button {
            1 => (DragType::Move, Corner::LeftTop),
            3 => {
                // We resize from whatever corner the pointer is
                // closest to.
                let mid_x = (st.width / 2) as i16;
                let mid_y = (st.height / 2) as i16;
                let corner = match (x >= mid_x, y >= mid_y) {
                    (false, false) => Corner::LeftTop,
                    (false, true) => Corner::LeftBottom,
                    (true, false) => Corner::RightTop,
                    (true, true) => Corner::RightBottom,
                };
                (DragType::Resize(corner), corner)
            }
            _ => {
                log::error!("Invalid button.");
                return;
            }
        };
        let (cx, cy) = corner.relative(st);
        let x = x - (cx as i16);
        let y = y - (cy as i16);
        self.drag = Some(Drag {
            type_,
            window,
            x,
            y,
        });
    }

    /// A button has been clicked.
    fn click(&self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        self.focus(window)?;
        self.raise(window)?;
        Ok(())
    }

    /// Dispatch on a CreateNotify event.
    fn create_notify(&mut self, ev: xproto::CreateNotifyEvent) -> Result<()>
    where
        Conn: Connection,
    {
        // TODO We should really factor all event handlers out into functions like this.
        let window = ev.window;
        self.clients.push(Client {
            window,
            state: if ev.override_redirect {
                None
            } else {
                Some(ClientState {
                    x: ev.x,
                    y: ev.y,
                    width: ev.width,
                    height: ev.height,
                    is_viewable: false,
                    wm_protocols: self.atoms.get_wm_protocols(&self.conn, window)?,
                    wm_state: Some(WmState {
                        state: WmStateState::Withdrawn,
                        icon: x11rb::NONE,
                    }),
                    wm_normal_hints: self.atoms.get_wm_normal_hints(&self.conn, window)?,
                })
            },
        });
        let client = self.clients.get(window);
        if !ev.override_redirect {
            self.manage(client)?;
        }
        Ok(())
    }

    /// Dispatch on a PropertyNotify event.
    fn property_notify(&mut self, ev: xproto::PropertyNotifyEvent) -> Result<()>
    where
        Conn: Connection,
    {
        let window = ev.window;
        if ev.atom == self.atoms.wm_protocols {
            log::debug!("Updating WM_PROTOCOLS.");
            self.clients
                .get_mut(window)
                .state
                .as_mut()
                .unwrap()
                .wm_protocols = self.atoms.get_wm_protocols(&self.conn, window)?;
        } else if ev.atom == self.atoms.wm_state {
            log::debug!("Updating WM_STATE.");
            self.clients
                .get_mut(window)
                .state
                .as_mut()
                .unwrap()
                .wm_state = self.atoms.get_wm_state(&self.conn, window)?;
        } else if ev.atom == xproto::AtomEnum::WM_NORMAL_HINTS.into() {
            log::debug!("Updating WM_NORMAL_HINTS.");
            self.clients
                .get_mut(window)
                .state
                .as_mut()
                .unwrap()
                .wm_normal_hints = self.atoms.get_wm_normal_hints(&self.conn, window)?
        } else {
            log::warn!("Ignoring.");
        }
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

    /// Kill a window.
    fn kill(&self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        let wm_protocols = &self
            .clients
            .get(window)
            .state
            .as_ref()
            .unwrap()
            .wm_protocols;
        if wm_protocols.delete_window {
            log::debug!("Client supports WM_DELETE_WINDOW; sending a message.");
            self.atoms.delete_window(&self.conn, window)?;
        } else {
            log::debug!("Client doesn't support WM_DELETE_WINDOW; killing directly.");
            self.conn.kill_client(window)?.check()?;
        }
        Ok(())
    }

    /// Begin managing a client.
    fn manage(&self, client: &Client) -> Result<()>
    where
        Conn: Connection,
    {
        let st = client.state.as_ref().unwrap();
        // Enforce our size policies.
        let (min_width, min_height) = st
            .wm_normal_hints
            .min_size
            .unwrap_or((MIN_WIDTH as i32, MIN_HEIGHT as i32));
        let (max_width, max_height) = st
            .wm_normal_hints
            .max_size
            .unwrap_or((MAX_WIDTH as i32, MAX_HEIGHT as i32));
        let mut value_list = xproto::ConfigureWindowAux::new()
            .width(st.width as u32)
            .height(st.height as u32);
        value_list.width = value_list
            .width
            .map(|w| w.max(min_width as u32).min(max_width as u32));
        value_list.height = value_list
            .height
            .map(|h| h.max(min_height as u32).min(max_height as u32));
        self.conn
            .configure_window(client.window, &value_list)?
            .check()?;

        // Do other stuff.
        let attrs = self.conn.get_window_attributes(client.window)?.reply()?;
        let state = match attrs.map_state {
            xproto::MapState::VIEWABLE => WmStateState::Normal,
            _ => WmStateState::Withdrawn,
        };
        self.atoms.set_wm_state(
            &self.conn,
            client.window,
            WmState {
                state,
                icon: x11rb::NONE,
            },
        )?;
        // Grab modifier + nothing.
        let nomod: u16 = 0;
        // TODO I don't fully understand sync/async grab modes.
        self.conn
            .grab_button(
                true,
                client.window,
                event_mask_to_u16(xproto::EventMask::BUTTON_PRESS),
                xproto::GrabMode::SYNC,
                xproto::GrabMode::SYNC,
                x11rb::NONE,
                x11rb::NONE,
                xproto::ButtonIndex::M1,
                nomod,
            )?
            .check()?;
        // Grab modifier + left mouse button.
        self.conn
            .grab_button(
                false,
                client.window,
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
            )?
            .check()?;
        // Grab modifier + right mouse button.
        self.conn
            .grab_button(
                false,
                client.window,
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
            )?
            .check()?;
        // Set our desired event mask.
        self.conn
            .change_window_attributes(
                client.window,
                &xproto::ChangeWindowAttributesAux::new().event_mask(
                    xproto::EventMask::ENTER_WINDOW
                        | xproto::EventMask::FOCUS_CHANGE
                        | xproto::EventMask::PROPERTY_CHANGE,
                ),
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

    // Actions go here. Note that, due to the need to conform to the Action
    // type, these functions' type signatures may sometimes seem odd.

    /// Kill the currently moused-over client.
    fn kill_focused_client(&mut self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        //X11 server sends window id of the window underneath the mouse cursor in
        //response to our bound keypress events. If the mouse is over the desktop
        //background the id we receive is 0. Ignore attempts to kill non-existant
        //windows.
        if 0 != window {
            self.kill(window)
        } else {
            Ok(())
        }
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

/// A corner.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
enum Corner {
    /// The top-left corner.
    LeftTop,
    /// The bottom-left corner.
    LeftBottom,
    /// The top-right corner.
    RightTop,
    /// The bottom-right corner.
    RightBottom,
}

impl Corner {
    /// Obtain the relative location of a corner for a given client window.
    fn relative(&self, st: &ClientState) -> (i16, i16) {
        match self {
            Self::LeftTop => (0, 0),
            Self::LeftBottom => (0, st.height as i16),
            Self::RightTop => (st.width as i16, 0),
            Self::RightBottom => (st.width as i16, st.height as i16),
        }
    }
}

/// A type of drag: either moving or resizing from a particular corner.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
enum DragType {
    /// A moving drag.
    Move,
    /// A resizing drag.
    Resize(Corner),
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

/// Run the window manager.
fn run_wm() -> Result<()> {
    log::debug!("Connecting to the X server.");
    let (conn, screen) = x11rb::connect(None)?;
    log::info!("Connected on screen {}.", screen);
    log::debug!("Initializing OxWM.");
    let oxwm = OxWM::new(conn, screen)?;
    log::debug!("Running OxWM.");
    oxwm.run()
}

/// Run the program.
fn main() -> Result<()> {
    simple_logger::SimpleLogger::new().init()?;
    run_wm()
}
