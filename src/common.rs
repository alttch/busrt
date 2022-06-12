#[cfg(feature = "rpc")]
use crate::Error;
#[cfg(feature = "rpc")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "rpc")]
use serde_value::Value;
#[cfg(feature = "rpc")]
use std::collections::HashMap;

#[cfg_attr(feature = "rpc", derive(Serialize, Deserialize))]
#[derive(Eq, PartialEq, Clone)]
pub struct ClientInfo<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub source: Option<&'a str>,
    pub port: Option<&'a str>,
    pub r_frames: u64,
    pub r_bytes: u64,
    pub w_frames: u64,
    pub w_bytes: u64,
    pub queue: usize,
    pub instances: usize,
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
#[cfg_attr(feature = "rpc", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct ClientList<'a> {
    #[cfg_attr(feature = "rpc", serde(borrow))]
    pub clients: Vec<ClientInfo<'a>>,
}

#[cfg_attr(feature = "rpc", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct BrokerStats {
    pub uptime: u64,
    pub r_frames: u64,
    pub r_bytes: u64,
    pub w_frames: u64,
    pub w_bytes: u64,
}

#[cfg_attr(feature = "rpc", derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct BrokerInfo<'a> {
    pub author: &'a str,
    pub version: &'a str,
}

#[allow(clippy::ptr_arg)]
#[cfg(feature = "rpc")]
pub fn str_to_params_map<'a>(s: &'a [&'a str]) -> Result<HashMap<&'a str, Value>, Error> {
    let mut params: HashMap<&str, Value> = HashMap::new();
    for pair in s {
        if !pair.is_empty() {
            let mut psp = pair.split('=');
            let var = psp
                .next()
                .ok_or_else(|| Error::data("var name not specified"))?;
            let v = psp
                .next()
                .ok_or_else(|| Error::data("var value not specified"))?;
            let value = if v == "false" {
                Value::Bool(false)
            } else if v == "true" {
                Value::Bool(true)
            } else if let Ok(i) = v.parse::<i64>() {
                Value::I64(i)
            } else if let Ok(f) = v.parse::<f64>() {
                Value::F64(f)
            } else {
                Value::String(v.to_owned())
            };
            params.insert(var, value);
        }
    }
    Ok(params)
}

#[cfg(feature = "broker")]
#[allow(clippy::cast_sign_loss)]
/// # Panics
///
/// Will panic if system clock is not available
pub fn now_ns() -> u64 {
    let t = nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME).unwrap();
    t.tv_sec() as u64 * 1_000_000_000 + t.tv_nsec() as u64
}
