use crate::{Error, Frame};
use std::fmt;

#[cfg(feature = "rpc")]
mod async_client;
#[cfg(feature = "rpc")]
#[allow(clippy::module_name_repetitions)]
pub use async_client::{DummyHandlers, Options, Rpc, RpcClient, RpcHandlers};

pub const RPC_NOTIFICATION: u8 = 0x00;
pub const RPC_REQUEST: u8 = 0x01;
pub const RPC_REPLY: u8 = 0x11;
pub const RPC_ERROR: u8 = 0x12;

pub const RPC_ERROR_CODE_NOT_FOUND: i16 = -32001;
pub const RPC_ERROR_CODE_PARSE: i16 = -32700;
pub const RPC_ERROR_CODE_INVALID_REQUEST: i16 = -32600;
pub const RPC_ERROR_CODE_METHOD_NOT_FOUND: i16 = -32601;
pub const RPC_ERROR_CODE_INVALID_METHOD_PARAMS: i16 = -32602;
pub const RPC_ERROR_CODE_INTERNAL: i16 = -32603;

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[repr(u8)]
pub enum RpcEventKind {
    Notification = RPC_NOTIFICATION,
    Request = RPC_REQUEST,
    Reply = RPC_REPLY,
    ErrorReply = RPC_ERROR,
}

#[allow(clippy::module_name_repetitions)]
#[inline]
pub fn rpc_err_str(v: impl fmt::Display) -> Option<Vec<u8>> {
    Some(v.to_string().as_bytes().to_vec())
}

impl fmt::Display for RpcEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RpcEventKind::Notification => "notifcation",
                RpcEventKind::Request => "request",
                RpcEventKind::Reply => "reply",
                RpcEventKind::ErrorReply => "error reply",
            }
        )
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct RpcEvent {
    kind: RpcEventKind,
    frame: Frame,
    payload_pos: usize,
    use_header: bool,
}

impl RpcEvent {
    #[inline]
    pub fn kind(&self) -> RpcEventKind {
        self.kind
    }
    #[inline]
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    #[inline]
    pub fn sender(&self) -> &str {
        self.frame.sender()
    }
    #[inline]
    pub fn primary_sender(&self) -> &str {
        self.frame.primary_sender()
    }
    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.frame().payload()[self.payload_pos..]
    }
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn id(&self) -> u32 {
        u32::from_le_bytes(
            if self.use_header {
                &self.frame.header().unwrap()[1..5]
            } else {
                &self.frame.payload()[1..5]
            }
            .try_into()
            .unwrap(),
        )
    }
    #[inline]
    pub fn is_response_required(&self) -> bool {
        self.id() != 0
    }
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn method(&self) -> &[u8] {
        if self.use_header {
            let header = self.frame.header.as_ref().unwrap();
            &header[5..header.len() - 1]
        } else {
            &self.frame().payload()[5..self.payload_pos - 1]
        }
    }
    #[inline]
    pub fn parse_method(&self) -> Result<&str, Error> {
        std::str::from_utf8(self.method()).map_err(Into::into)
    }
    /// # Panics
    ///
    /// Should not panic
    #[inline]
    pub fn code(&self) -> i16 {
        if self.kind == RpcEventKind::ErrorReply {
            i16::from_le_bytes(
                if self.use_header {
                    &self.frame.header().unwrap()[5..7]
                } else {
                    &self.frame.payload()[5..7]
                }
                .try_into()
                .unwrap(),
            )
        } else {
            0
        }
    }
}

impl TryFrom<Frame> for RpcEvent {
    type Error = Error;
    fn try_from(frame: Frame) -> Result<Self, Self::Error> {
        let (body, use_header) = frame
            .header()
            .map_or_else(|| (frame.payload(), false), |h| (h, true));
        if body.is_empty() {
            Err(Error::data("Empty RPC frame"))
        } else {
            macro_rules! check_len {
                ($len: expr) => {
                    if body.len() < $len {
                        return Err(Error::data("Invalid RPC frame"));
                    }
                };
            }
            match body[0] {
                RPC_NOTIFICATION => Ok(RpcEvent {
                    kind: RpcEventKind::Notification,
                    frame,
                    payload_pos: usize::from(!use_header),
                    use_header: false,
                }),
                RPC_REQUEST => {
                    check_len!(6);
                    if use_header {
                        Ok(RpcEvent {
                            kind: RpcEventKind::Request,
                            frame,
                            payload_pos: 0,
                            use_header: true,
                        })
                    } else {
                        let mut sp = body[5..].splitn(2, |c| *c == 0);
                        let method = sp.next().ok_or_else(|| Error::data("No RPC method"))?;
                        let payload_pos = 6 + method.len();
                        sp.next()
                            .ok_or_else(|| Error::data("No RPC params block"))?;
                        Ok(RpcEvent {
                            kind: RpcEventKind::Request,
                            frame,
                            payload_pos,
                            use_header: false,
                        })
                    }
                }
                RPC_REPLY => {
                    check_len!(5);
                    Ok(RpcEvent {
                        kind: RpcEventKind::Reply,
                        frame,
                        payload_pos: if use_header { 0 } else { 5 },
                        use_header,
                    })
                }
                RPC_ERROR => {
                    check_len!(7);
                    Ok(RpcEvent {
                        kind: RpcEventKind::ErrorReply,
                        frame,
                        payload_pos: if use_header { 0 } else { 7 },
                        use_header,
                    })
                }
                v => Err(Error::data(format!("Unsupported RPC frame code {}", v))),
            }
        }
    }
}

#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct RpcError {
    code: i16,
    data: Option<Vec<u8>>,
}

impl TryFrom<&RpcEvent> for RpcError {
    type Error = Error;
    #[inline]
    fn try_from(event: &RpcEvent) -> Result<Self, Self::Error> {
        if event.kind() == RpcEventKind::ErrorReply {
            Ok(RpcError::new(event.code(), Some(event.payload().to_vec())))
        } else {
            Err(Error::data("not a RPC error"))
        }
    }
}

impl RpcError {
    #[inline]
    pub fn new(code: i16, data: Option<Vec<u8>>) -> Self {
        Self { code, data }
    }
    #[inline]
    pub fn code(&self) -> i16 {
        self.code
    }
    #[inline]
    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }
    #[inline]
    pub fn method(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_METHOD_NOT_FOUND,
            data: err,
        }
    }
    #[inline]
    pub fn not_found(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_NOT_FOUND,
            data: err,
        }
    }
    #[inline]
    pub fn params(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_INVALID_METHOD_PARAMS,
            data: err,
        }
    }
    #[inline]
    pub fn parse(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_PARSE,
            data: err,
        }
    }
    #[inline]
    pub fn invalid(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_INVALID_REQUEST,
            data: err,
        }
    }
    #[inline]
    pub fn internal(err: Option<Vec<u8>>) -> Self {
        Self {
            code: RPC_ERROR_CODE_INTERNAL,
            data: err,
        }
    }
    /// Converts displayable to Vec<u8>
    #[inline]
    pub fn convert_data(v: impl fmt::Display) -> Vec<u8> {
        v.to_string().as_bytes().to_vec()
    }
}

impl From<Error> for RpcError {
    #[inline]
    fn from(e: Error) -> RpcError {
        RpcError {
            code: -32000 - e.kind() as i16,
            data: None,
        }
    }
}

#[cfg(feature = "broker-rpc")]
impl From<rmp_serde::encode::Error> for RpcError {
    #[inline]
    fn from(e: rmp_serde::encode::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_INTERNAL,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl From<regex::Error> for RpcError {
    #[inline]
    fn from(e: regex::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_PARSE,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl From<std::io::Error> for RpcError {
    #[inline]
    fn from(e: std::io::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_INTERNAL,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

#[cfg(feature = "broker-rpc")]
impl From<rmp_serde::decode::Error> for RpcError {
    #[inline]
    fn from(e: rmp_serde::decode::Error) -> RpcError {
        RpcError {
            code: RPC_ERROR_CODE_PARSE,
            data: Some(e.to_string().as_bytes().to_vec()),
        }
    }
}

impl fmt::Display for RpcError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rpc error code: {}", self.code)
    }
}

impl std::error::Error for RpcError {}

#[allow(clippy::module_name_repetitions)]
pub type RpcResult = Result<Option<Vec<u8>>, RpcError>;

#[inline]
pub(crate) fn prepare_call_payload(method: &str, id_bytes: &[u8]) -> Vec<u8> {
    let m = method.as_bytes();
    let mut payload = Vec::with_capacity(m.len() + 6);
    payload.push(RPC_REQUEST);
    payload.extend(id_bytes);
    payload.extend(m);
    payload.push(0x00);
    payload
}
