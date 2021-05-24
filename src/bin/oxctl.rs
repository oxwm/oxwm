use essrpc::transports::BincodeTransport;
use essrpc::RPCClient;

use std::os::unix::net::UnixStream;

use structopt::StructOpt;

use oxwm::*;

#[derive(StructOpt)]
#[structopt(about = "control OxWM")]
enum Opt {
    Ls,
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    let stream = UnixStream::connect("/tmp/oxwm")?;
    let client = OxWMRPCClient::new(BincodeTransport::new(stream));
    match opt {
        Ls => println!("{:?}", client.ls()),
    }
    Ok(())
}
