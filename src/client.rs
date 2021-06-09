//! Local data about the state of the X server.

use x11rb::connection::Connection;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt as _;

use crate::atom::*;
use crate::Result;

/// Local data about a top-level window.
#[derive(Clone, Debug)]
pub(crate) struct Client {
    /// The client window.
    pub(crate) window: xproto::Window,
    /// The client's state. We don't keep track of this for windows with
    /// override-redirect set.
    pub(crate) state: Option<ClientState>,
}

impl Client {
    /// Indicates whether a window has its override-redirect flag set.
    pub(crate) fn override_redirect(&self) -> bool {
        self.state.is_some()
    }
}

/// Local data about the state of a top-level window.
#[derive(Clone, Debug)]
pub(crate) struct ClientState {
    /// Horizontal position.
    pub(crate) x: i16,
    /// Vertical position.
    pub(crate) y: i16,
    /// Horizontal extent.
    pub(crate) width: u16,
    /// Vertical extent.
    pub(crate) height: u16,
    /// Whether the window is viewable.
    pub(crate) is_viewable: bool,
    /// The client's WM_PROTOCOLS.
    pub(crate) wm_protocols: WmProtocols,
    /// The client's WM_STATE.
    pub(crate) wm_state: Option<WmState>,
    /// The client's WM_NORMAL_HINTS.
    pub(crate) wm_normal_hints: WmSizeHints,
}

/// Local data about the state of all top-level windows. This includes windows
/// that have the override-redirect flag set; however, for such windows, we
/// don't track any local properties. (In particular, we need to keep track of
/// the stacking order for all windows.)
///
/// You can essentially think of this as a simulator for a tiny subset of the X
/// protocol. The point is that keeping track of the state of the server lets us
/// use our own knowledge when making decisions, rather than having to
/// constantly issue queries.
///
/// The user is responsible for the following.
/// * It is an error to try to insert two clients with the same window ID.
/// * It is an error to try to perform an operation on a window ID for which
///   there is no corresponding client.
#[derive(Clone, Debug)]
pub(crate) struct Clients {
    /// The window stack.
    // It feels wrong to use a vector, since we're going to be inserting and
    // removing elements---but Bjarne Stroustrup says that it takes a
    // ridiculously large n for linked lists to be faster than vectors (and
    // Rust's standard linked list type doesn't even have insert/remove
    // methods). Not to mention, a linked list wouldn't even be asymptotically
    // better, since it still takes O(n) to determine where to insert/remove.
    stack: Vec<Client>,
    /// The currently-focused window, if any (the root window doesn't count).
    focus: Option<xproto::Window>,
}

impl Clients {
    /// Get the currently-focused client.
    pub(crate) fn get_focus(&self) -> Option<&Client> {
        let window = self.focus?;
        Some(self.get(window))
    }

    /// Get the currently-focused client.
    #[allow(dead_code)]
    pub(crate) fn get_focus_mut(&mut self) -> Option<&mut Client> {
        let window = self.focus?;
        Some(self.get_mut(window))
    }

    /// Set the currently-focused client.
    pub(crate) fn set_focus<A>(&mut self, window: A)
    where
        A: Into<Option<xproto::Window>>,
    {
        let window = window.into();
        debug_assert!(window
            .map(|w| self.stack.iter().any(|c| c.window == w))
            .unwrap_or(true));
        self.focus = window;
    }

    /// Get a client by its window.
    pub(crate) fn get(&self, window: xproto::Window) -> &Client {
        self.get_with_index(window).1
    }

    /// Get a client by its window.
    pub(crate) fn get_mut(&mut self, window: xproto::Window) -> &mut Client {
        self.get_with_index_mut(window).1
    }

    /// Indicates whether a client corresponding to the given window exists.
    pub(crate) fn has_client(&self, window: xproto::Window) -> bool {
        self.stack.iter().any(|client| client.window == window)
    }

    /// Get an iterator over the stack, from bottom to top.
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Client> {
        self.stack.iter()
    }

    /// Get an iterator over the stack, from bottom to top.
    pub(crate) fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Client> {
        self.stack.iter_mut()
    }

    /// Move a client to just above another one.
    pub(crate) fn move_to_above(&mut self, window: xproto::Window, sibling: xproto::Window) {
        let (i, _) = self.get_with_index(window);
        if i > 0 && self.stack[i - 1].window == sibling {
            return;
        }
        let client = self.stack.remove(i);
        let (j, _) = self.get_with_index(sibling);
        self.stack.insert(j + 1, client);
    }

    /// Lower a client to the bottom of the stack.
    pub(crate) fn move_to_bottom(&mut self, window: xproto::Window) {
        if self.stack.first().unwrap().window == window {
            return;
        }
        let (i, _) = self.get_with_index(window);
        let client = self.stack.remove(i);
        self.stack.insert(0, client);
    }

    /// Raise a client to the top of the stack.
    #[allow(dead_code)]
    pub(crate) fn move_to_top(&mut self, window: xproto::Window) {
        if self.top().window == window {
            return;
        }
        let (i, _) = self.get_with_index(window);
        let client = self.stack.remove(i);
        self.stack.push(client)
    }

    /// Initialize a new client stack by issuing queries to the server.
    pub(crate) fn new<Conn>(conn: &Conn, screen: usize, atoms: &Atoms) -> Result<Self>
    where
        Conn: Connection,
    {
        let root = conn.setup().roots[screen].root;
        let mut stack = Vec::new();
        let children = conn.query_tree(root)?.reply()?.children;
        // Fortunately, the server is guaranteed to return the windows in
        // stacking order, from bottom to top.
        for window in children {
            let attrs = conn.get_window_attributes(window)?.reply()?;
            let override_redirect = attrs.override_redirect;
            let state = if override_redirect {
                None
            } else {
                let geom = conn.get_geometry(window)?.reply()?;
                let is_viewable = attrs.map_state == xproto::MapState::VIEWABLE;
                let wm_protocols = atoms.get_wm_protocols(conn, window)?;
                let wm_state = atoms.get_wm_state(conn, window)?;
                let wm_normal_hints = atoms.get_wm_normal_hints(conn, window)?;
                Some(ClientState {
                    x: geom.x,
                    y: geom.y,
                    width: geom.width,
                    height: geom.height,
                    is_viewable,
                    wm_protocols,
                    wm_state,
                    wm_normal_hints,
                })
            };
            stack.push(Client { window, state })
        }
        let focus = conn.get_input_focus()?.reply()?.focus;
        let focus = if stack.iter().find(|client| client.window == focus).is_none() {
            None
        } else {
            Some(focus)
        };
        Ok(Clients { stack, focus })
    }

    /// Push a client on top of the stack.
    pub(crate) fn push(&mut self, client: Client) {
        debug_assert!(!self.stack.iter().any(|c| c.window == client.window));
        self.stack.push(client);
    }

    /// Remove a client from the stack.
    pub(crate) fn remove(&mut self, window: xproto::Window) {
        self.stack.remove(self.get_with_index(window).0);
        if self.focus == Some(window) {
            self.focus = None;
        }
    }

    /// Get the client that is on the top of the stack.
    pub(crate) fn top(&self) -> &Client {
        self.stack.last().unwrap()
    }

    /// Get the client that is on the top of the stack.
    #[allow(dead_code)]
    pub(crate) fn top_mut(&mut self) -> &mut Client {
        self.stack.last_mut().unwrap()
    }

    // Private methods

    /// Get the `Client` that corresponds to a given window, along with its
    /// index.
    fn get_with_index(&self, window: xproto::Window) -> (usize, &Client) {
        self.iter()
            .enumerate()
            .find(|(_, c)| c.window == window)
            .unwrap()
    }

    /// Get the `Client` that corresponds to a given window, along with its
    /// index.
    fn get_with_index_mut(&mut self, window: xproto::Window) -> (usize, &mut Client) {
        self.iter_mut()
            .enumerate()
            .find(|(_, c)| c.window == window)
            .unwrap()
    }
}

/// Tests.
#[test]
fn can_remove_focused_window() {
    let mut clients = Clients {
        stack: vec![],
        focus: None,
    };

    clients.push(Client {
        window: 100,
        state: Some(ClientState {
            x: 1,
            y: 1,
            width: 10,
            height: 10,
            is_viewable: true,
            wm_protocols: WmProtocols::new(),
            wm_state: None,
            wm_normal_hints: WmSizeHints::new(),
        }),
    });

    clients.push(Client {
        window: 200,
        state: Some(ClientState {
            x: 1,
            y: 1,
            width: 10,
            height: 10,
            is_viewable: true,
            wm_protocols: WmProtocols::new(),
            wm_state: None,
            wm_normal_hints: WmSizeHints::new(),
        }),
    });

    clients.push(Client {
        window: 250,
        state: Some(ClientState {
            x: 1,
            y: 1,
            width: 10,
            height: 10,
            is_viewable: false,
            wm_protocols: WmProtocols::new(),
            wm_state: None,
            wm_normal_hints: WmSizeHints::new(),
        }),
    });

    clients.push(Client {
        window: 300,
        state: Some(ClientState {
            x: 1,
            y: 1,
            width: 10,
            height: 10,
            is_viewable: true,
            wm_protocols: WmProtocols::new(),
            wm_state: None,
            wm_normal_hints: WmSizeHints::new(),
        }),
    });

    clients.set_focus(300);
    assert_eq!(clients.get_focus().unwrap().window, 300);

    clients.remove(100);
    assert_eq!(clients.get_focus().unwrap().window, 300);

    clients.remove(300);
    assert!(clients.get_focus().is_none());

    clients.set_focus(200);
    clients.remove(250);
    assert_eq!(clients.get_focus().unwrap().window, 200);

    clients.remove(200);
    assert!(clients.get_focus().is_none());
}
