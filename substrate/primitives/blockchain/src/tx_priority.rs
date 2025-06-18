use std::marker::PhantomData;
use codec::{Decode, Encode};
use serde::Deserialize;
use sp_runtime::transaction_validity::TransactionPriority;
use std::path::Path;
use sp_api::__private::BlockT;
use sp_core::H160;

pub trait TransactionDetailProvider: Send + Sync {
    type Block: BlockT;
    fn get_transaction_detail(&self, tx: <Self::Block as BlockT>::Extrinsic) -> Option<TransactionDetail>;
}
pub struct NoDetailProvider<B: BlockT>(PhantomData<B>);
impl<B: BlockT> TransactionDetailProvider for NoDetailProvider<B> {
    type Block = B;
    fn get_transaction_detail(&self, _tx: <Self::Block as BlockT>::Extrinsic) -> Option<TransactionDetail> {
        None
    }
}
impl<B: BlockT> NoDetailProvider<B> {
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
    pub tx_name_provider: Box<dyn TransactionDetailProvider<Block = Block>>,
}

impl<Block: BlockT> TransactionPriorityModule<Block> {
    pub fn new(tx_priority_list: &Path, tx_name_provider: Box<dyn TransactionDetailProvider<Block = Block>>) -> Self {
        let file = std::fs::File::open(tx_priority_list).unwrap();
        let reader = std::io::BufReader::new(file);
        let data: Vec<TransactionPriorityItem> = serde_json::from_reader(reader).unwrap();

        Self { priority_list: data,
            tx_name_provider
        }
    }

    pub fn get_priority(&self, tx_detail: TransactionDetail) -> Option<TransactionPriority> {
        let priority_item = self.priority_list.iter().find(|item| item.module == tx_detail.module && item.extrinsic == tx_detail.extrinsic);
        if let Some(item) = priority_item {
            Some(item.priority)
        } else { None }
    }
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct SubstrateTransactionDetail {
    pub signer: H160,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct EvmTransactionDetail {
    pub call_address: H160,
    pub signer: Option<H160>,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub enum TransactionTypeDetail {
    Substrate(SubstrateTransactionDetail),
    Evm(EvmTransactionDetail)
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct  TransactionDetail {
    module: String,
    extrinsic: String,
    transaction_data: Option<TransactionTypeDetail>,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct TransactionPriorityItem {
    module: String,
    extrinsic: String,
    transaction_data: Option<TransactionTypeDetail>,
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

    // let l = TransactionPriorityModule::new(Path::new("src/my_list.json"), Box::new(NoNameProvider::new()));
}

// example file my_list.json

// [
// {
// "module": "Balances",
// "extrinsic": "transfer_allow_death",
// "transaction_data": null,
// "priority": 7
// }
// ]

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