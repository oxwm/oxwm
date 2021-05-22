use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use x11rb::protocol::xproto;

/// Local data about a top-level window.
#[derive(Debug, Deserialize, Serialize)]
pub struct Client {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub name: String,
}

/// Local data about top-level windows.
#[derive(Debug, Deserialize, Serialize)]
pub struct Clients {
    clients: HashMap<xproto::Window, Client>,
}

impl Clients {
    pub fn new() -> Clients {
        Clients {
            clients: HashMap::new(),
        }
    }

    pub fn add(&mut self, window: xproto::Window, client: Client) {
        if self.clients.contains_key(&window) {
            return;
        }
        self.clients.insert(window, client);
    }

    /// Set local client data.
    pub fn configure(&mut self, window: xproto::Window, x: i16, y: i16, width: u16, height: u16) {
        if let Some(client) = self.clients.get_mut(&window) {
            client.x = x;
            client.y = y;
            client.width = width;
            client.height = height;
        }
    }

    /// Remove a window from the managed set.
    pub fn remove(&mut self, window: xproto::Window) {
        self.clients.remove(&window);
    }
}
