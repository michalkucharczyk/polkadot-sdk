use codec::{Decode, Encode};
use serde::Deserialize;
use sp_runtime::transaction_validity::TransactionPriority;
use serde_json::from_str;
use std::path::Path;
use sp_core::H160;

pub trait TransactionNameProvider {
    fn get_transaction_name(&self) -> Option<(Vec<u8>, Vec<u8>)>;
}
pub struct NameProvider;
impl TransactionNameProvider for NameProvider {
    fn get_transaction_name(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        None
    }
}
impl NameProvider {
    pub fn new() -> Self {
        Self
    }
}
pub trait TransactionPriorityModuleT {
    fn get_priority(&self) -> Option<TransactionPriority>;
}

pub struct TransactionPriorityModule {
    priority_list: Vec<TransactionPriorityItem>,
}

impl TransactionPriorityModule {
    pub fn new(tx_priority_list: Option<&Path>) -> Self {
        if let Some(path) = tx_priority_list {
            let file = std::fs::File::open(path).unwrap();
            let reader = std::io::BufReader::new(file);
            let data: Vec<TransactionPriorityItem> = serde_json::from_reader(reader).unwrap();
            Self { priority_list: data }

        } else {
            Self { priority_list: vec![] }
        }
    }
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct SubstrateTransactionDetail {
    index: u32,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct EvmTransactionDetail {
    call_address: H160,
    signer: Option<H160>,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub enum TransactionDetail {
    Evm(EvmTransactionDetail),
    Substrate(SubstrateTransactionDetail)
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct TransactionPriorityItem {
    transaction: TransactionDetail,
    priority: TransactionPriority,
}

#[test]
fn test_me() {
    // let path = std::env::current_dir().unwrap();
    // println!("The current directory is {}", path.display());

    let file = std::fs::File::open(Path::new("src/my_list.json")).unwrap();
    let reader = std::io::BufReader::new(file);
    let u: Vec<TransactionPriorityItem> = serde_json::from_reader(reader).unwrap();

    println!("- - - {:?}",u);

    let l = TransactionPriorityModule::new(Some(Path::new("src/my_list.json")));
}

// example file my_list.json
// [
// {
// "transaction":{
// "Substrate":{
// "index": 4
// }},"priority": 7
// },
// {
// "transaction": {
// "Evm": {
// "call_address": "0x7e878d91757ee4e599109fa861909f177e7785b0",
// "signer": null
// }},
// "priority": 123
// }
// ]