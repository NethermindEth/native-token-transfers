//! Typed wrapper around the Soroban RPC `getEvents` method.
//!
//! Polls `getEvents` for a contract id, decodes the base64 XDR topic and
//! data fields into [`ScVal`]s, and exposes typed accessors so tests can
//! assert on event shape without string-matching base64. Use
//! [`EventQuery::find_with_topic`] to wait for a specific event symbol to
//! show up — RPC indexing lags ledger close by a few seconds and a single
//! fetch is not reliable.

use std::{thread, time::Duration};

use serde_json::Value;
use stellar_xdr::curr::{Limits, ReadXdr, ScMapEntry, ScVal};

use crate::{cli, ctx::TestContext};

/// One Soroban contract event with its topics and data decoded out of XDR.
pub struct DecodedEvent {
    pub topics: Vec<ScVal>,
    pub data: ScVal,
    pub ledger: u32,
    pub tx_hash: String,
    pub contract_id: String,
}

impl DecodedEvent {
    /// Returns the topic at `index` as a UTF-8 symbol string, if it is one.
    /// Topic 0 is conventionally the event-name symbol (e.g.
    /// `"transfer_sent"`).
    pub fn topic_symbol(&self, index: usize) -> Option<String> {
        symbol_string(self.topics.get(index)?)
    }

    /// Returns the value of the named field from the event's data map, if
    /// the data is a map and the field exists.
    pub fn data_field(&self, name: &str) -> Option<&ScVal> {
        let ScVal::Map(Some(entries)) = &self.data else {
            return None;
        };
        entries
            .0
            .iter()
            .find(|ScMapEntry { key, .. }| symbol_eq(key, name))
            .map(|e| &e.val)
    }

    /// Returns the topic at `index` as a `u32`, if it is one.
    pub fn topic_u32(&self, i: usize) -> Option<u32> {
        match self.topics.get(i)? {
            ScVal::U32(n) => Some(*n),
            _ => None,
        }
    }

    /// Returns the named data field as a `u32`, if present and the right
    /// ScVal variant.
    pub fn data_u32(&self, name: &str) -> Option<u32> {
        match self.data_field(name)? {
            ScVal::U32(n) => Some(*n),
            _ => None,
        }
    }

    /// Returns the named data field as a `u64`, if present and the right
    /// ScVal variant.
    pub fn data_u64(&self, name: &str) -> Option<u64> {
        match self.data_field(name)? {
            ScVal::U64(n) => Some(*n),
            _ => None,
        }
    }
}

/// Builder for an RPC `getEvents` query scoped to a single contract id.
pub struct EventQuery<'a> {
    ctx: &'a TestContext,
    contract_id: String,
    start_ledger: Option<u32>,
}

impl<'a> EventQuery<'a> {
    pub fn new(ctx: &'a TestContext, contract_id: impl Into<String>) -> Self {
        Self {
            ctx,
            contract_id: contract_id.into(),
            start_ledger: None,
        }
    }

    /// Overrides the auto-derived start ledger (latest minus 200) — useful
    /// when the test knows the exact ledger range of interest.
    pub fn from_ledger(mut self, start: u32) -> Self {
        self.start_ledger = Some(start);
        self
    }

    /// Single-shot fetch: returns all events the RPC has indexed for this
    /// contract since `start_ledger`, decoded.
    pub fn fetch(&self) -> Vec<DecodedEvent> {
        let latest = cli::rpc_latest_ledger(&self.ctx.rpc_url);
        let start = self
            .start_ledger
            .unwrap_or_else(|| latest.saturating_sub(200).max(1));
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getEvents","params":{{"startLedger":{start},"filters":[{{"type":"contract","contractIds":["{cid}"]}}],"pagination":{{"limit":100}}}}}}"#,
            cid = self.contract_id,
        );
        let resp = cli::rpc_call(&self.ctx.rpc_url, &body);
        let arr = resp["result"]["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        arr.iter().filter_map(decode_event).collect()
    }

    /// Polls [`fetch`] every second until an event whose first topic is
    /// `symbol` shows up, or `timeout` elapses. Required because RPC
    /// indexing lags ledger close — a single fetch right after a tx will
    /// often miss the event the tx emitted.
    pub fn find_with_topic(&self, symbol: &str, timeout: Duration) -> Option<DecodedEvent> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            for ev in self.fetch() {
                if ev.topic_symbol(0).as_deref() == Some(symbol) {
                    return Some(ev);
                }
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Decodes the topic array of a raw `getEvents` event JSON entry into
/// `ScVal`s, skipping any that fail to parse.
pub fn decode_topics(raw_event: &Value) -> Vec<ScVal> {
    let arr = raw_event["topic"].as_array().cloned().unwrap_or_default();
    arr.iter()
        .filter_map(|t| t.as_str())
        .filter_map(|s| ScVal::from_xdr_base64(s, Limits::none()).ok())
        .collect()
}

fn decode_event(raw: &Value) -> Option<DecodedEvent> {
    let topics = decode_topics(raw);
    if topics.is_empty() {
        return None;
    }
    let val_str = raw["value"].as_str()?;
    let data = ScVal::from_xdr_base64(val_str, Limits::none()).ok()?;
    Some(DecodedEvent {
        topics,
        data,
        ledger: raw["ledger"].as_u64()? as u32,
        tx_hash: raw["txHash"].as_str()?.to_string(),
        contract_id: raw["contractId"].as_str()?.to_string(),
    })
}

fn symbol_string(v: &ScVal) -> Option<String> {
    let ScVal::Symbol(sym) = v else { return None };
    let bytes: &[u8] = sym.0.as_ref();
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

fn symbol_eq(v: &ScVal, expected: &str) -> bool {
    let ScVal::Symbol(sym) = v else { return false };
    let bytes: &[u8] = sym.0.as_ref();
    bytes == expected.as_bytes()
}
