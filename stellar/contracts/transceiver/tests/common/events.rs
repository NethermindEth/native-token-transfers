use soroban_sdk::{testutils::Events as _, xdr, Address, Bytes, Env, Map, Symbol, TryFromVal, Val};

/// Reads the `payload` of the last message the wormhole core published — the
/// exact bytes the transceiver posted (an outbound envelope or an Accountant
/// broadcast). Lets tests decode and assert the on-the-wire message the real
/// core received, since it stores the payload rather than exposing a getter.
pub fn posted_payload(env: &Env, core: &Address) -> Bytes {
    let events = env.events().all().filter_by_contract(core);
    let event = events.events().last().expect("a published message");
    let xdr::ContractEventBody::V0(body) = &event.body;
    let fields: Map<Symbol, Val> = Map::try_from_val(env, &body.data).expect("event data is a map");
    let payload = fields.get(Symbol::new(env, "payload")).expect("payload field");
    Bytes::try_from_val(env, &payload).expect("payload is bytes")
}
