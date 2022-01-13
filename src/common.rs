#[cfg(feature = "serialization")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
#[derive(Eq, PartialEq)]
pub struct ClientInfo<'a> {
    pub name: &'a str,
    pub tp: &'a str,
    pub source: Option<&'a str>,
    pub port: Option<&'a str>,
}
impl<'a> Ord for ClientInfo<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(other.name)
    }
}
impl<'a> PartialOrd for ClientInfo<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct ClientList<'a> {
    #[cfg_attr(feature = "serialization", serde(borrow))]
    pub clients: Vec<ClientInfo<'a>>,
}
