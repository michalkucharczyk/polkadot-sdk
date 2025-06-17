use std::marker::PhantomData;
use codec::{Decode, Encode};
use serde::Deserialize;
use sp_runtime::transaction_validity::TransactionPriority;
use std::path::Path;
use sp_api::__private::BlockT;
use sp_core::H160;

pub trait TransactionNameProvider: Send + Sync {
    type Block: BlockT;
    fn get_transaction_name(&self, tx: <Self::Block as BlockT>::Extrinsic) -> Option<(Vec<u8>, Vec<u8>)>;
}
pub struct NoNameProvider<B: BlockT>(PhantomData<B>);
impl<B: BlockT> TransactionNameProvider for NoNameProvider<B> {
    type Block = B;
    fn get_transaction_name(&self, _tx: <Self::Block as BlockT>::Extrinsic) -> Option<(Vec<u8>, Vec<u8>)> {
        None
    }
}
impl<B: BlockT> NoNameProvider<B> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}
pub trait TransactionPriorityModuleT {
    type Block: BlockT;
    fn get_priority(&self, tx: <Self::Block as BlockT>::Extrinsic) -> Option<TransactionPriority>;
}

pub struct TransactionPriorityModule<Block> {
    priority_list: Vec<TransactionPriorityItem>,
    pub tx_name_provider: Box<dyn TransactionNameProvider<Block = Block>>,
}

impl<Block: BlockT> TransactionPriorityModule<Block> {
    pub fn new(tx_priority_list: &Path, tx_name_provider: Box<dyn TransactionNameProvider<Block = Block>>) -> Self {
        let file = std::fs::File::open(tx_priority_list).unwrap();
        let reader = std::io::BufReader::new(file);
        let data: Vec<TransactionPriorityItem> = serde_json::from_reader(reader).unwrap();

        Self { priority_list: data,
            tx_name_provider
        }
    }

    pub fn get_priority(&self, module: Vec<u8>, extrinsic: Vec<u8>) -> Option<TransactionPriority> {
        let priority_item = self.priority_list.iter().find(|item| item.module == module && item.extrinsic == extrinsic);
        if let Some(item) = priority_item {
            Some(item.priority)
        } else { None }
    }
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct SubstrateTransactionDetail {
    data: Vec<u8>,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct EvmTransactionDetail {
    call_address: H160,
    signer: Option<H160>,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub enum TransactionDetail {
    Substrate(SubstrateTransactionDetail),
    Evm(EvmTransactionDetail)
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct TransactionPriorityItem {
    module: Vec<u8>,
    extrinsic: Vec<u8>,
    transaction: Option<TransactionDetail>,
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