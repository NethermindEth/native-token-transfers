use std::rc::Rc;

use soroban_sdk::{
    token::TokenClient,
    xdr::{
        AccountEntry, AccountEntryExt, AccountId, LedgerEntry, LedgerEntryData, LedgerEntryExt,
        LedgerKey, LedgerKeyAccount, PublicKey, ScAddress, SequenceNumber, Thresholds, Uint256,
    },
    Address, Env, TryFromVal,
};

/// Deploys the native XLM SAC and credits `to` with `amount` stroops.
///
/// XLM can't be minted, so the stroops come from an account whose ledger entry
/// is written directly, as `register_stellar_asset_contract_v2` writes its issuer.
pub fn deploy_xlm<'a>(env: &Env, to: &Address, amount: i64) -> TokenClient<'a> {
    let account_id = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256([1; 32])));
    let key = LedgerKey::Account(LedgerKeyAccount {
        account_id: account_id.clone(),
    });
    let entry = LedgerEntry {
        data: LedgerEntryData::Account(AccountEntry {
            account_id: account_id.clone(),
            balance: amount,
            flags: 0,
            home_domain: Default::default(),
            inflation_dest: None,
            num_sub_entries: 0,
            seq_num: SequenceNumber(0),
            thresholds: Thresholds([1; 4]),
            signers: Default::default(),
            ext: AccountEntryExt::V0,
        }),
        last_modified_ledger_seq: 0,
        ext: LedgerEntryExt::V0,
    };
    env.host()
        .add_ledger_entry(&Rc::new(key), &Rc::new(entry), None)
        .unwrap();

    let xlm = TokenClient::new(env, &env.deployer().with_stellar_asset([0u8; 4]).deploy());
    let account = Address::try_from_val(env, &ScAddress::Account(account_id)).unwrap();
    xlm.transfer(&account, to, &i128::from(amount));
    xlm
}
