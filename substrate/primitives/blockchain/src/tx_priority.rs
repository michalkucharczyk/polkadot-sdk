use codec::{Decode, Encode};
use serde::Deserialize;
use sp_runtime::transaction_validity::TransactionPriority;
use std::path::Path;
use sp_api::__private::BlockT;
use sp_core::H160;

/// Provides transaction details required by `TransactionPriorityModule`.
pub trait TransactionDetailProvider: Send + Sync {
    type Block: BlockT;
    fn get_transaction_detail(&self, tx: <Self::Block as BlockT>::Extrinsic) -> Option<TransactionDetail>;
}

/// Implemented for the `Client`, to get tx priorities from the transaction pool.
pub trait TransactionPriorityModuleT {
    type Block: BlockT;
    fn get_priority(&self, tx: <Self::Block as BlockT>::Extrinsic) -> Option<TransactionPriority>;
}

/// Main struct for getting TX priority from the specified list.
pub struct TransactionPriorityModule<Block> {
    priority_list: Vec<TransactionPriorityItem>,
    pub tx_detail_provider: Box<dyn TransactionDetailProvider<Block = Block>>,
}

impl<Block: BlockT> TransactionPriorityModule<Block> {
    pub fn new(tx_priority_list: &Path, tx_detail_provider: Box<dyn TransactionDetailProvider<Block = Block>>) -> Self {
        let file = std::fs::File::open(tx_priority_list).unwrap();
        let reader = std::io::BufReader::new(file);
        let data: Vec<TransactionPriorityItem> = serde_json::from_reader(reader).unwrap();

        Self { priority_list: data,
            tx_detail_provider
        }
    }

    pub fn get_priority(&self, tx_detail: TransactionDetail) -> Option<TransactionPriority> {
        let priority_item = self.priority_list.iter().find(|item| item.module == tx_detail.module && item.extrinsic == tx_detail.extrinsic);
        if let Some(item) = priority_item {
            Some(item.priority)
        } else { None }
    }
}

/// Transaction details specific for an EVM transaction
#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct EvmTransactionDetail {
    pub call_address: Option<H160>,
    pub signer: H160,
}

#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub enum TransactionTypeDetail {
    Evm(EvmTransactionDetail)
}

/// Transaction details used to determine transaction's priority.
#[derive(Clone, Debug)]
pub struct  TransactionDetail {
    pub module: &'static str,
    pub extrinsic: &'static str,
    pub transaction_data: Option<TransactionTypeDetail>,
}

/// Data type of the `tx_priority_list` json file.
#[derive(Clone, Encode, Decode, Deserialize, Debug)]
pub struct TransactionPriorityItem {
    module: String,
    extrinsic: String,
    transaction_data: Option<TransactionTypeDetail>,
    priority: TransactionPriority,
}
