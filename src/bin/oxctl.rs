use oxwm::*;

use serde::Deserialize;
use serde::Serialize;

use essrpc::transports::BincodeTransport;
use essrpc::RPCClient;

use std::os::unix::net::UnixStream;

use structopt::StructOpt;

use x11rb::protocol::xproto;

use oxwm::*;

// #[derive(Clone, Debug, Deserialize, Serialize)]
// pub enum Delta<T> {
//     Absolute(T),
//     Relative(T),
// }

// impl<T> FromStr for Delta<T>
// where
//     T: FromStr,
// {
//     type Err = T::Err;
//     fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
//         let is_relative = s.starts_with('+') || s.starts_with('-');
//         let payload = T::from_str(s)?;
//         Ok(if is_relative {
//             Delta::Relative(payload)
//         } else {
//             Delta::Absolute(payload)
//         })
//     }
// }

#[derive(Clone, Debug, Serialize, Deserialize, StructOpt)]
#[structopt(about = "control OxWM")]
pub enum Opts {
    Ls,
    Mv {
        window: xproto::Window,
        x: i32,
        y: i32,
    },
}
use Opts::*;

fn main() -> Result<()> {
    let opts = Opts::from_args();
    println!("{:?}", opts);
    let stream = UnixStream::connect("/tmp/oxwm")?;
    let client = OxRPCClient::new(BincodeTransport::new(stream));
    match opts {
        Ls => println!("{:?}", client.ls()?),
        Mv { window, x, y } => client.configure_window(
            window,
            // x.unwrap_or(Delta::Relative(0)),
            // y.unwrap_or(Delta::Relative(0)),
            Some(x),
            Some(y),
            None,
            None,
            None,
            None,
            None,
        )?,
    }
    Ok(())
}
