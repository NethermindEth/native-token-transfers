use soroban_sdk::{contractclient, Address, Bytes, BytesN, Env};

#[contractclient(name = "TransceiverClient")]
pub trait TransceiverInterface {
    fn get_manager(env: Env) -> Address;
    fn get_manager_id(env: Env) -> BytesN<32>;
    fn send_message(
        env: Env,
        recipient_chain: u32,
        recipient_manager: BytesN<32>,
        manager_payload: Bytes,
    );
}
