#![ doc = include_str!( concat!( env!( "CARGO_MANIFEST_DIR" ), "/", "README.md" ) ) ]

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "rpc")]
pub use async_trait::async_trait;

pub const OP_NOP: u8 = 0x00;
pub const OP_PUBLISH: u8 = 0x01;
pub const OP_SUBSCRIBE: u8 = 0x02;
pub const OP_UNSUBSCRIBE: u8 = 0x03;
pub const OP_EXCLUDE: u8 = 0x04;
pub const OP_UNEXCLUDE: u8 = 0x05;
pub const OP_MESSAGE: u8 = 0x12;
pub const OP_BROADCAST: u8 = 0x13;
pub const OP_ACK: u8 = 0xFE;

pub const PROTOCOL_VERSION: u16 = 0x01;

pub const RESPONSE_OK: u8 = 0x01;

pub const PING_FRAME: &[u8] = &[0, 0, 0, 0, 0, 0, 0, 0, 0];

pub const ERR_CLIENT_NOT_REGISTERED: u8 = 0x71;
pub const ERR_DATA: u8 = 0x72;
pub const ERR_IO: u8 = 0x73;
pub const ERR_OTHER: u8 = 0x74;
pub const ERR_NOT_SUPPORTED: u8 = 0x75;
pub const ERR_BUSY: u8 = 0x76;
pub const ERR_NOT_DELIVERED: u8 = 0x77;
pub const ERR_TIMEOUT: u8 = 0x78;
pub const ERR_ACCESS: u8 = 0x79;

pub const GREETINGS: [u8; 1] = [0xEB];

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub static AUTHOR: &str = "(c) 2022 Bohemia Automation / Altertech";

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);
pub const DEFAULT_BUF_TTL: Duration = Duration::from_micros(10);
pub const DEFAULT_BUF_SIZE: usize = 8192;

pub const DEFAULT_QUEUE_SIZE: usize = 8192;

pub const SECONDARY_SEP: &str = "%%";

/// When a frame is sent, methods do not wait for the result, but they return OpConfirm type to let
/// the sender get the result if required.
///
/// When the frame is sent with QoS "processed", the Option contains Receiver<Result>
///
/// Example:
///
/// ```rust,ignore
/// use busrt::QoS;
///
/// let result = client.send("target", payload, QoS::Processed).await.unwrap(); // get send result
/// let confirm = result.unwrap(); // get OpConfirm
/// let op_result = confirm.await.unwrap(); // receive the operation result
/// match op_result {
///     Ok(_) => { /* the server has confirmed that it had processed the message */ }
///     Err(e) => { /* the server has returned an error */ }
/// }
/// ```
pub type OpConfirm = Option<tokio::sync::oneshot::Receiver<Result<(), Error>>>;
pub type Frame = Arc<FrameData>;
pub type EventChannel = async_channel::Receiver<Frame>;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum ErrorKind {
    NotRegistered = ERR_CLIENT_NOT_REGISTERED,
    NotSupported = ERR_NOT_SUPPORTED,
    Io = ERR_IO,
    Timeout = ERR_TIMEOUT,
    Data = ERR_DATA,
    Busy = ERR_BUSY,
    NotDelivered = ERR_NOT_DELIVERED,
    Access = ERR_ACCESS,
    Other = ERR_OTHER,
    Eof = 0xff,
}

impl From<u8> for ErrorKind {
    fn from(code: u8) -> Self {
        match code {
            ERR_CLIENT_NOT_REGISTERED => ErrorKind::NotRegistered,
            ERR_NOT_SUPPORTED => ErrorKind::NotSupported,
            ERR_IO => ErrorKind::Io,
            ERR_DATA => ErrorKind::Data,
            ERR_BUSY => ErrorKind::Busy,
            ERR_NOT_DELIVERED => ErrorKind::NotDelivered,
            ERR_ACCESS => ErrorKind::Access,
            _ => ErrorKind::Other,
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ErrorKind::NotRegistered => "Client not registered",
                ErrorKind::NotSupported => "Feature not supported",
                ErrorKind::Io => "I/O Error",
                ErrorKind::Timeout => "Timeout",
                ErrorKind::Data => "Data Error",
                ErrorKind::Busy => "Busy",
                ErrorKind::NotDelivered => "Frame not delivered",
                ErrorKind::Other => "Error",
                ErrorKind::Access => "Access denied",
                ErrorKind::Eof => "Eof",
            }
        )
    }
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: Option<String>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref message) = self.message {
            write!(f, "{}: {}", self.kind, message)
        } else {
            write!(f, "{}", self.kind)
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    #[inline]
    pub fn new(kind: ErrorKind, message: Option<impl fmt::Display>) -> Self {
        Self {
            kind,
            message: message.map(|m| m.to_string()),
        }
    }
    #[inline]
    pub fn io(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: Some(e.to_string()),
        }
    }
    #[inline]
    pub fn data(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Data,
            message: Some(e.to_string()),
        }
    }
    #[inline]
    pub fn access(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Access,
            message: Some(e.to_string()),
        }
    }
    #[inline]
    pub fn not_supported(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::NotSupported,
            message: Some(e.to_string()),
        }
    }
    #[inline]
    pub fn not_registered() -> Self {
        Self {
            kind: ErrorKind::NotRegistered,
            message: None,
        }
    }
    #[inline]
    pub fn not_delivered() -> Self {
        Self {
            kind: ErrorKind::NotDelivered,
            message: None,
        }
    }
    #[inline]
    pub fn timeout() -> Self {
        Self {
            kind: ErrorKind::Timeout,
            message: None,
        }
    }
    #[inline]
    pub fn busy(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Busy,
            message: Some(e.to_string()),
        }
    }
    #[inline]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

pub trait IntoBusRtResult {
    fn to_busrt_result(self) -> Result<(), Error>;
}

impl IntoBusRtResult for u8 {
    #[inline]
    fn to_busrt_result(self) -> Result<(), Error> {
        if self == RESPONSE_OK {
            Ok(())
        } else {
            Err(Error {
                kind: self.into(),
                message: None,
            })
        }
    }
}

impl From<tokio::time::error::Elapsed> for Error {
    fn from(_e: tokio::time::error::Elapsed) -> Error {
        Error::timeout()
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        if e.kind() == std::io::ErrorKind::UnexpectedEof
            || e.kind() == std::io::ErrorKind::BrokenPipe
            || e.kind() == std::io::ErrorKind::ConnectionReset
        {
            Error {
                kind: ErrorKind::Eof,
                message: None,
            }
        } else {
            Error::io(e)
        }
    }
}

impl From<&std::io::Error> for Error {
    fn from(e: &std::io::Error) -> Error {
        if e.kind() == std::io::ErrorKind::UnexpectedEof
            || e.kind() == std::io::ErrorKind::BrokenPipe
            || e.kind() == std::io::ErrorKind::ConnectionReset
        {
            Error {
                kind: ErrorKind::Eof,
                message: None,
            }
        } else {
            Error::io(e)
        }
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Error {
        Error::data(e)
    }
}

impl From<std::array::TryFromSliceError> for Error {
    fn from(e: std::array::TryFromSliceError) -> Error {
        Error::data(e)
    }
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(_e: async_channel::SendError<T>) -> Error {
        Error {
            kind: ErrorKind::Eof,
            message: None,
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for Error {
    fn from(_e: tokio::sync::oneshot::error::RecvError) -> Error {
        Error {
            kind: ErrorKind::Eof,
            message: None,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum FrameOp {
    Nop = OP_NOP,
    Message = OP_MESSAGE,
    Broadcast = OP_BROADCAST,
    PublishTopic = OP_PUBLISH,
    SubscribeTopic = OP_SUBSCRIBE,
    UnsubscribeTopic = OP_UNSUBSCRIBE,
    ExcludeTopic = OP_EXCLUDE,
    // not include but unexclude as it's clearly an opposite operation to exclude
    // while include may be confused with subscribe
    UnexcludeTopic = OP_UNEXCLUDE,
}

impl TryFrom<u8> for FrameOp {
    type Error = Error;
    fn try_from(tp: u8) -> Result<Self, Error> {
        match tp {
            OP_NOP => Ok(FrameOp::Nop),
            OP_MESSAGE => Ok(FrameOp::Message),
            OP_BROADCAST => Ok(FrameOp::Broadcast),
            OP_PUBLISH => Ok(FrameOp::PublishTopic),
            OP_SUBSCRIBE => Ok(FrameOp::SubscribeTopic),
            OP_UNSUBSCRIBE => Ok(FrameOp::UnsubscribeTopic),
            OP_EXCLUDE => Ok(FrameOp::ExcludeTopic),
            OP_UNEXCLUDE => Ok(FrameOp::UnexcludeTopic),
            _ => Err(Error::data(format!("Invalid frame type: {}", tp))),
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum QoS {
    No = 0,
    Processed = 1,
    Realtime = 2,
    RealtimeProcessed = 3,
}

impl QoS {
    #[inline]
    pub fn is_realtime(self) -> bool {
        self as u8 & 0b10 != 0
    }
    #[inline]
    pub fn needs_ack(self) -> bool {
        self as u8 & 0b1 != 0
    }
}

impl TryFrom<u8> for QoS {
    type Error = Error;
    fn try_from(q: u8) -> Result<Self, Error> {
        match q {
            0 => Ok(QoS::No),
            1 => Ok(QoS::Processed),
            2 => Ok(QoS::Realtime),
            3 => Ok(QoS::RealtimeProcessed),
            _ => Err(Error::data(format!("Invalid QoS: {}", q))),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum FrameKind {
    Prepared = 0xff,
    Message = OP_MESSAGE,
    Broadcast = OP_BROADCAST,
    Publish = OP_PUBLISH,
    Acknowledge = OP_ACK,
    Nop = OP_NOP,
}

impl TryFrom<u8> for FrameKind {
    type Error = Error;
    fn try_from(code: u8) -> Result<Self, Self::Error> {
        match code {
            OP_MESSAGE => Ok(FrameKind::Message),
            OP_BROADCAST => Ok(FrameKind::Broadcast),
            OP_PUBLISH => Ok(FrameKind::Publish),
            OP_ACK => Ok(FrameKind::Acknowledge),
            OP_NOP => Ok(FrameKind::Nop),
            _ => Err(Error::data(format!("Invalid frame type: {:x}", code))),
        }
    }
}

#[derive(Debug)]
pub struct FrameData {
    kind: FrameKind,
    sender: Option<String>,
    topic: Option<String>,
    header: Option<Vec<u8>>, // zero-copy payload prefix
    buf: Vec<u8>,
    payload_pos: usize,
    realtime: bool,
}

impl FrameData {
    #[inline]
    pub fn new(
        kind: FrameKind,
        sender: Option<String>,
        topic: Option<String>,
        header: Option<Vec<u8>>,
        buf: Vec<u8>,
        payload_pos: usize,
        realtime: bool,
    ) -> Self {
        Self {
            kind,
            sender,
            topic,
            header,
            buf,
            payload_pos,
            realtime,
        }
    }
    #[inline]
    pub fn new_nop() -> Self {
        Self {
            kind: FrameKind::Nop,
            sender: None,
            topic: None,
            header: None,
            buf: Vec::new(),
            payload_pos: 0,
            realtime: false,
        }
    }
    #[inline]
    pub fn kind(&self) -> FrameKind {
        self.kind
    }
    /// # Panics
    ///
    /// Will panic if called for a prepared frame
    #[inline]
    pub fn sender(&self) -> &str {
        self.sender.as_ref().unwrap()
    }
    /// # Panics
    ///
    /// Will panic if called for a prepared frame
    #[inline]
    pub fn primary_sender(&self) -> &str {
        let primary_sender = self.sender.as_ref().unwrap();
        if let Some(pos) = primary_sender.find(SECONDARY_SEP) {
            &primary_sender[..pos]
        } else {
            primary_sender
        }
    }
    /// Filled for pub/sub communications
    #[inline]
    pub fn topic(&self) -> Option<&str> {
        self.topic.as_deref()
    }
    /// To keep zero-copy model, frames contain the full incoming buffer + actual payload position.
    /// Use this method to get the actual call payload.
    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.buf[self.payload_pos..]
    }
    /// The header can be used by certain implementations (e.g. the default RPC layer) to
    /// keep zero-copy model. The header is None for IPC communications, but filled for
    /// inter-thread ones. A custom layer should use/parse the header to avoid unnecessary payload
    /// copy
    #[inline]
    pub fn header(&self) -> Option<&[u8]> {
        self.header.as_deref()
    }
    #[inline]
    pub fn is_realtime(&self) -> bool {
        self.realtime
    }
}

pub mod borrow;
pub mod common;
pub mod tools {
    #[cfg(any(feature = "rpc", feature = "broker", feature = "ipc"))]
    pub mod pubsub;
}

#[cfg(feature = "broker")]
pub mod broker;
#[cfg(feature = "cursors")]
pub mod cursors;
#[cfg(feature = "ipc")]
pub mod ipc;
#[cfg(feature = "rpc")]
pub mod rpc;

#[cfg(any(feature = "rpc", feature = "broker", feature = "ipc"))]
pub mod client;
#[cfg(any(feature = "broker", feature = "ipc"))]
pub mod comm;
