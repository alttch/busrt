// TODO example: client
// TODO example: client listener
// TODO example: rpc client with no handlers
// TODO example: rpc client with custom handlers
// TODO example: sync python
// TODO example: async python
// TODO js client (rpc)
// TODO CLI tool: send messages, listen, send rpc, listen rpc, benchmark
// TODO broker internal broadcast (topic) events
// TODO broker info events
// TODO tests
// TODO rpc tests
// TODO bus rpc: stats
// TODO release
// TODO release python libs
use std::fmt;
use std::sync::Arc;

pub const OP_NOP: u8 = 0x00;
pub const OP_PUBLISH: u8 = 0x01;
pub const OP_SUBSCRIBE: u8 = 0x02;
pub const OP_UNSUBSCRIBE: u8 = 0x03;
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

pub const GREETINGS: [u8; 1] = [0xEB];

/// When the frame is sent, methods do not wait for the result but return OpConfirm type to let the
/// sender get it if required.
///
/// When the frame is sent with QoS > 0, the Option contains Receiver<Result>
///
/// Example:
///
/// ```rust,ignore
/// use elbus::QoS;
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

impl Error {
    pub fn new(kind: ErrorKind, message: Option<impl fmt::Display>) -> Self {
        Self {
            kind,
            message: message.map(|m| m.to_string()),
        }
    }
    pub fn io(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: Some(e.to_string()),
        }
    }
    pub fn data(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Data,
            message: Some(e.to_string()),
        }
    }
    pub fn not_supported(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::NotSupported,
            message: Some(e.to_string()),
        }
    }
    pub fn not_registered() -> Self {
        Self {
            kind: ErrorKind::NotRegistered,
            message: None,
        }
    }
    pub fn timeout() -> Self {
        Self {
            kind: ErrorKind::Timeout,
            message: None,
        }
    }
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

pub trait IntoElbusResult {
    fn to_elbus_result(self) -> Result<(), Error>;
}

impl IntoElbusResult for u8 {
    #[inline]
    fn to_elbus_result(self) -> Result<(), Error> {
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
            _ => Err(Error::data(format!("Invalid frame type: {}", tp))),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum QoS {
    No = 0,
    Processed = 1,
}

impl TryFrom<u8> for QoS {
    type Error = Error;
    fn try_from(q: u8) -> Result<Self, Error> {
        match q {
            0 => Ok(QoS::No),
            1 => Ok(QoS::Processed),
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
}

impl FrameData {
    pub fn new(
        kind: FrameKind,
        sender: Option<String>,
        topic: Option<String>,
        header: Option<Vec<u8>>,
        buf: Vec<u8>,
        payload_pos: usize,
    ) -> Self {
        Self {
            kind,
            sender,
            topic,
            header,
            buf,
            payload_pos,
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
    #[inline]
    /// Filled for pub/sub communications
    pub fn topic(&self) -> &Option<String> {
        &self.topic
    }
    #[inline]
    /// To keep zero-copy model, frames contain the full incoming buffer + actual payload position.
    /// Use this method to get the actual call payload.
    pub fn payload(&self) -> &[u8] {
        &self.buf[self.payload_pos..]
    }
    #[inline]
    /// The header can be used by certain implementations (e.g. the default RPC layer) to
    /// keep zero-copy model. The header is None for IPC communications, but filled for
    /// inter-thread ones. A custom layer should use/parse the header to avoid unnecessary payload
    /// copy
    pub fn header(&self) -> Option<&[u8]> {
        self.header.as_deref()
    }
}

#[cfg(feature = "broker")]
pub mod broker;
#[cfg(feature = "ipc")]
pub mod ipc;
#[cfg(any(feature = "rpc", feature = "broker-api"))]
pub mod rpc;

pub mod borrow;
#[cfg(any(feature = "rpc", feature = "broker", feature = "ipc"))]
pub mod client;
