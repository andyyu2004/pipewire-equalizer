use core::fmt;
use std::error::Error as StdError;
use std::io;

pub struct Error {
    err: Box<ErrorImpl>,
}

impl Error {
    pub(crate) fn io(error: io::Error) -> Self {
        Error {
            err: Box::new(ErrorImpl {
                code: ErrorCode::Io(error),
                line: 0,
                column: 0,
            }),
        }
    }

    pub(crate) fn syntax(code: ErrorCode, line: usize, column: usize) -> Self {
        Error {
            err: Box::new(ErrorImpl { code, line, column }),
        }
    }

    pub(crate) fn fix_position<F>(self, f: F) -> Self
    where
        F: FnOnce(ErrorCode) -> Error,
    {
        if self.err.line == 0 {
            f(self.err.code)
        } else {
            self
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

struct ErrorImpl {
    code: ErrorCode,
    line: usize,
    column: usize,
}

pub(crate) enum ErrorCode {
    /// EOF while parsing a list.
    EofWhileParsingList,

    /// EOF while parsing an object.
    EofWhileParsingObject,

    /// EOF while parsing a string.
    EofWhileParsingString,

    /// EOF while parsing a JSON value.
    EofWhileParsingValue,

    /// Expected this character to be a `':'`.
    ExpectedColon,

    /// Expected this character to be either a `','` or a `']'`.
    ExpectedListCommaOrEnd,

    /// Expected this character to be either a `','` or a `'}'`.
    ExpectedObjectCommaOrEnd,

    /// Expected to parse either a `true`, `false`, or a `null`.
    ExpectedSomeIdent,

    /// Expected this character to start a JSON value.
    ExpectedSomeValue,

    /// Expected this character to be a `"`.
    ExpectedDoubleQuote,

    /// Invalid hex escape code.
    InvalidEscape,

    /// Invalid number.
    InvalidNumber,

    /// Number is bigger than the maximum value of its type.
    NumberOutOfRange,

    /// Invalid unicode code point.
    InvalidUnicodeCodePoint,

    /// Control character found while parsing a string.
    ControlCharacterWhileParsingString,

    /// Object key is not a string.
    KeyMustBeAString,

    /// Contents of key were supposed to be a number.
    ExpectedNumericKey,

    /// Object key is a non-finite float value.
    FloatKeyMustBeFinite,

    /// Lone leading surrogate in hex escape.
    LoneLeadingSurrogateInHexEscape,

    /// JSON has a comma after the last value in an array or map.
    TrailingComma,

    /// JSON has non-whitespace trailing characters after the value.
    TrailingCharacters,

    /// Unexpected end of hex escape.
    UnexpectedEndOfHexEscape,

    /// Encountered nesting of JSON maps and arrays more than 128 layers deep.
    RecursionLimitExceeded,
    Io(io::Error),
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.err.code {
            ErrorCode::Io(err) => err.source(),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&*self.err, f)
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error {
            err: Box::new(ErrorImpl {
                code: ErrorCode::Io(io::Error::other(msg.to_string())),
                line: 0,
                column: 0,
            }),
        }
    }
}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error {
            err: Box::new(ErrorImpl {
                code: ErrorCode::Io(io::Error::other(msg.to_string())),
                line: 0,
                column: 0,
            }),
        }
    }
}

impl fmt::Display for ErrorImpl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.line == 0 {
            fmt::Display::fmt(&self.code, f)
        } else {
            write!(
                f,
                "{} at line {} column {}",
                self.code, self.line, self.column
            )
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErrorCode::Io(err) => fmt::Display::fmt(err, f),
            ErrorCode::EofWhileParsingList => f.write_str("EOF while parsing a list"),
            ErrorCode::EofWhileParsingObject => f.write_str("EOF while parsing an object"),
            ErrorCode::EofWhileParsingString => f.write_str("EOF while parsing a string"),
            ErrorCode::EofWhileParsingValue => f.write_str("EOF while parsing a value"),
            ErrorCode::ExpectedColon => f.write_str("expected `:`"),
            ErrorCode::ExpectedListCommaOrEnd => f.write_str("expected `,` or `]`"),
            ErrorCode::ExpectedObjectCommaOrEnd => f.write_str("expected `,` or `}`"),
            ErrorCode::ExpectedSomeIdent => f.write_str("expected ident"),
            ErrorCode::ExpectedSomeValue => f.write_str("expected value"),
            ErrorCode::ExpectedDoubleQuote => f.write_str("expected `\"`"),
            ErrorCode::InvalidEscape => f.write_str("invalid escape"),
            ErrorCode::InvalidNumber => f.write_str("invalid number"),
            ErrorCode::NumberOutOfRange => f.write_str("number out of range"),
            ErrorCode::InvalidUnicodeCodePoint => f.write_str("invalid unicode code point"),
            ErrorCode::ControlCharacterWhileParsingString => {
                f.write_str("control character (\\u0000-\\u001F) found while parsing a string")
            }
            ErrorCode::KeyMustBeAString => f.write_str("key must be a string"),
            ErrorCode::ExpectedNumericKey => {
                f.write_str("invalid value: expected key to be a number in quotes")
            }
            ErrorCode::FloatKeyMustBeFinite => {
                f.write_str("float key must be finite (got NaN or +/-inf)")
            }
            ErrorCode::LoneLeadingSurrogateInHexEscape => {
                f.write_str("lone leading surrogate in hex escape")
            }
            ErrorCode::TrailingComma => f.write_str("trailing comma"),
            ErrorCode::TrailingCharacters => f.write_str("trailing characters"),
            ErrorCode::UnexpectedEndOfHexEscape => f.write_str("unexpected end of hex escape"),
            ErrorCode::RecursionLimitExceeded => f.write_str("recursion limit exceeded"),
        }
    }
}
