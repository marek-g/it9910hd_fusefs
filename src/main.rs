mod it9910hd_driver;
use it9910hd_driver::*;

fn main() {
    match run() {
        Err(err) => panic!("Cannot open Encoder: {}", err),
        _ => (),
    };
}
