////////////////
// local mods //
////////////////
mod args;
mod command;

use std::{env, process, io};

use log::{debug, info, error};
use structopt::StructOpt;

use crate::args::AppOptions;

fn main() {
    let _ = env_logger::init();
    let options =  AppOptions::from_args();
    if options.verbose_level > 1 {
        debug!("options: {:#?}", &options);
    }
    match command::parallel_exec_command(&options) {
        Ok(n) => info!("completed handling {} items", n),
        Err(crate::command::CommandError::CommandError(rc, err_msg)) => {
            eprint!("{}", err_msg);
            process::exit(rc);
        },
	Err(command::CommandError::IoError(ref e)) if e.kind() == io::ErrorKind::BrokenPipe => {
            if options.verbose_level > 0 {
                debug!("Ignoring broken pipe error");
            }
        },
        Err(e @ command::CommandError::SendError(command::SendChannel::Out)) => {
            if options.verbose_level > 0 {
                debug!("Ignoring send error to out channel. Err: {:?}", e);
            }
        },
        Err(err) => {
            let message = format!("{:?}", &err);
            if env::var("RUST_LOG").is_ok() {
                error!("{}", message);
            } else {
                eprintln!("{}", message);
            }
            panic!("{}", message);
        }
    }
}
