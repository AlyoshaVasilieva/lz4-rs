#![feature(old_io)]
#![feature(std_misc)]
#![feature(libc)]

pub mod liblz4;
pub mod decoder;
pub mod encoder;

pub use decoder::*;
pub use encoder::*;
pub use liblz4::version;