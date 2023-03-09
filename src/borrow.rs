use std::sync::Arc;

/// When a frame is sent via sockets, only the data pointer is necessary. For inter-thread
/// communications, a full data block is requied. This smart-pointer type acts like
/// std::borrow:Cow, except is stuck to &[u8] / Vec<u8> buffers
///
/// The principle is simple: always give the full data block if possible, but give a pointer if
/// isn't
///
/// Example:
///
/// ```rust
/// use busrt::borrow::Cow;
///
/// let owned_payload: Cow = vec![0u8, 1, 2, 3].into();
/// let borrowed_payload: Cow = vec![0u8, 1, 2, 3].as_slice().into();
/// ```
#[derive(Clone)]
pub enum Cow<'a> {
    Borrowed(&'a [u8]),
    Owned(Vec<u8>),
    Referenced(Arc<Vec<u8>>),
}

impl<'a> From<Vec<u8>> for Cow<'a> {
    fn from(src: Vec<u8>) -> Cow<'a> {
        Cow::Owned(src)
    }
}

impl<'a> From<Arc<Vec<u8>>> for Cow<'a> {
    fn from(src: Arc<Vec<u8>>) -> Cow<'a> {
        Cow::Referenced(src)
    }
}

impl<'a> From<&'a [u8]> for Cow<'a> {
    fn from(src: &'a [u8]) -> Cow<'a> {
        Cow::Borrowed(src)
    }
}

impl<'a> Cow<'a> {
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Cow::Borrowed(v) => v,
            Cow::Owned(v) => v.as_slice(),
            Cow::Referenced(v) => v.as_slice(),
        }
    }
    #[inline]
    pub fn to_vec(self) -> Vec<u8> {
        match self {
            Cow::Borrowed(v) => v.to_vec(),
            Cow::Owned(v) => v,
            Cow::Referenced(v) => v.to_vec(),
        }
    }
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Cow::Borrowed(v) => v.len(),
            Cow::Owned(v) => v.len(),
            Cow::Referenced(v) => v.len(),
        }
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Cow::Borrowed(v) => v.is_empty(),
            Cow::Owned(v) => v.is_empty(),
            Cow::Referenced(v) => v.is_empty(),
        }
    }
}
