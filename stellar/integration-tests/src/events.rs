use std::{thread, time::Duration};

use serde_json::Value;
use stellar_xdr::curr::{Limits, ReadXdr, ScMapEntry, ScVal};

use crate::ctx::TestContext;

pub struct DecodedEvent {
    pub topics: Vec<ScVal>,
    pub data: ScVal,
    pub ledger: u32,
    pub tx_hash: String,
    pub contract_id: String,
}

impl DecodedEvent {
    pub fn topic_symbol(&self, index: usize) -> Option<String> {
        symbol_string(self.topics.get(index)?)
    }

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
}

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

    pub fn from_ledger(mut self, start: u32) -> Self {
        self.start_ledger = Some(start);
        self
    }

    pub fn fetch(&self) -> Vec<DecodedEvent> {
        let latest = rpc_latest_ledger(&self.ctx.rpc_url);
        let start = self
            .start_ledger
            .unwrap_or_else(|| latest.saturating_sub(200).max(1));
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getEvents","params":{{"startLedger":{start},"filters":[{{"type":"contract","contractIds":["{cid}"]}}],"pagination":{{"limit":100}}}}}}"#,
            cid = self.contract_id,
        );
        let resp = rpc_call(&self.ctx.rpc_url, &body);
        let arr = resp["result"]["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        arr.iter().filter_map(decode_event).collect()
    }

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

fn rpc_latest_ledger(rpc_url: &str) -> u32 {
    let body =
        r#"{"jsonrpc":"2.0","id":1,"method":"getLatestLedger","params":{}}"#;
    let resp = rpc_call(rpc_url, body);
    resp["result"]["sequence"]
        .as_u64()
        .expect("ledger sequence missing") as u32
}

fn rpc_call(rpc_url: &str, body: &str) -> Value {
    let out = std::process::Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            rpc_url,
            "-H",
            "Content-Type: application/json",
            "-d",
            body,
        ])
        .output()
        .expect("failed to spawn curl");
    if !out.status.success() {
        panic!(
            "curl failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).expect("RPC response not JSON")
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
