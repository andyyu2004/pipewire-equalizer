mod de;
pub mod error;
mod iter;
mod macros;
mod number;
mod read;
mod ser;
mod value;

pub use self::de::{from_reader, from_slice, from_str};
pub use self::error::{Error, Result};
pub use self::ser::{
    to_string, to_string_pretty, to_vec, to_vec_pretty, to_writer, to_writer_pretty,
};
pub use self::value::{Value, to_value};

macro_rules! tri {
    ($e:expr $(,)?) => {
        match $e {
            core::result::Result::Ok(val) => val,
            core::result::Result::Err(err) => return core::result::Result::Err(err),
        }
    };
}

use tri;
