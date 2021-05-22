use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(about = "control OxWM")]
enum Subcommand {
    Ls,
}

fn main() {}
