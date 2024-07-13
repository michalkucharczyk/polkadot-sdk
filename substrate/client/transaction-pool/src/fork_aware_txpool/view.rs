// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Transaction pool view.
//!
//! The View represents the state of the transaction pool at given block. The view is created when
//! new block is notified to transaction pool. Views are removed on finalization.

use crate::{
	graph::{
		self, watcher::Watcher, ExtrinsicFor, ExtrinsicHash, ValidatedTransaction,
		ValidatedTransactionFor,
	},
	log_xt_debug,
};
use std::{collections::HashMap, sync::Arc, time::Instant};

use parking_lot::Mutex;
use sc_transaction_pool_api::{PoolStatus, TransactionSource};
use sp_runtime::{traits::Block as BlockT, transaction_validity::TransactionValidityError};

use crate::LOG_TARGET;
use sp_blockchain::HashAndNumber;

pub(super) struct RevalidationResult<ChainApi: graph::ChainApi> {
	revalidated: HashMap<ExtrinsicHash<ChainApi>, ValidatedTransactionFor<ChainApi>>,
	invalid_hashes: Vec<ExtrinsicHash<ChainApi>>,
}

/// Used to obtain result from RevalidationWorker on View side.
pub(super) type RevalidationResultReceiver<ChainApi> =
	tokio::sync::mpsc::Receiver<RevalidationResult<ChainApi>>;

/// Used to send revalidation result from RevalidationWorker to View.
pub(super) type RevalidationResultSender<ChainApi> =
	tokio::sync::mpsc::Sender<RevalidationResult<ChainApi>>;

/// Used to receive finish-revalidation-request from View on RevalidationWorker side.
pub(super) type FinishRevalidationRequestReceiver = tokio::sync::mpsc::Receiver<()>;

/// Used to send finish-revalidation-request from View to RevalidationWorker.
pub(super) type FinishRevalidationRequestSender = tokio::sync::mpsc::Sender<()>;

/// Endpoints of channels used on View side (maintain thread)
pub(super) struct FinishRevalidationLocalChannels<ChainApi: graph::ChainApi> {
	/// Used to send finish revalidation request.
	finish_revalidation_request_tx: Option<FinishRevalidationRequestSender>,
	/// Used to receive revalidation results.
	revalidation_result_rx: RevalidationResultReceiver<ChainApi>,
}

/// Endpoints of channels used on RevalidationWorker side (background thread)
impl<ChainApi: graph::ChainApi> FinishRevalidationLocalChannels<ChainApi> {
	fn new(
		finish_revalidation_request_tx: FinishRevalidationRequestSender,
		revalidation_result_rx: RevalidationResultReceiver<ChainApi>,
	) -> Self {
		Self {
			finish_revalidation_request_tx: Some(finish_revalidation_request_tx),
			revalidation_result_rx,
		}
	}

	fn remove_sender(&mut self) {
		self.finish_revalidation_request_tx = None;
	}
}

/// Endpoints of channels used on RevalidationWorker side (background thread)
pub(super) struct FinishRevalidationWorkerChannels<ChainApi: graph::ChainApi> {
	/// Used to receive finish revalidation request.
	finish_revalidation_request_rx: FinishRevalidationRequestReceiver,
	/// Used to send revalidation results.
	revalidation_result_tx: RevalidationResultSender<ChainApi>,
}

impl<ChainApi: graph::ChainApi> FinishRevalidationWorkerChannels<ChainApi> {
	fn new(
		finish_revalidation_request_rx: FinishRevalidationRequestReceiver,
		revalidation_result_tx: RevalidationResultSender<ChainApi>,
	) -> Self {
		Self { finish_revalidation_request_rx, revalidation_result_tx }
	}
}

/// Represents the state of transaction for given block.
pub(super) struct View<ChainApi: graph::ChainApi> {
	pub(super) pool: graph::Pool<ChainApi>,
	pub(super) at: HashAndNumber<ChainApi::Block>,

	/// Endpoints of communication channel with background worker.
	revalidation_worker_channels: Mutex<Option<FinishRevalidationLocalChannels<ChainApi>>>,
}

impl<ChainApi> View<ChainApi>
where
	ChainApi: graph::ChainApi,
	<ChainApi::Block as BlockT>::Hash: Unpin,
{
	/// Creates a new empty view.
	pub(super) fn new(
		api: Arc<ChainApi>,
		at: HashAndNumber<ChainApi::Block>,
		options: graph::Options,
	) -> Self {
		Self {
			pool: graph::Pool::new(options, true.into(), api),
			at,
			revalidation_worker_channels: Mutex::from(None),
		}
	}

	/// Creates a copy of the other view.
	pub(super) fn new_from_other(&self, at: &HashAndNumber<ChainApi::Block>) -> Self {
		View {
			at: at.clone(),
			pool: self.pool.deep_clone(),
			revalidation_worker_channels: Mutex::from(None),
		}
	}

	/// Imports many unvalidate extrinsics into the view.
	pub(super) async fn submit_many(
		&self,
		source: TransactionSource,
		xts: impl IntoIterator<Item = ExtrinsicFor<ChainApi>>,
	) -> Vec<Result<ExtrinsicHash<ChainApi>, ChainApi::Error>> {
		let xts = xts.into_iter().collect::<Vec<_>>();
		log_xt_debug!(target: LOG_TARGET, xts.iter().map(|xt| self.pool.validated_pool().api().hash_and_length(xt).0), "[{:?}] view::submit_many at:{}", self.at.hash);
		self.pool.submit_at(&self.at, source, xts).await
	}

	/// Import a single extrinsic and starts to watch its progress in the view.
	pub(super) async fn submit_and_watch(
		&self,
		source: TransactionSource,
		xt: ExtrinsicFor<ChainApi>,
	) -> Result<Watcher<ExtrinsicHash<ChainApi>, ExtrinsicHash<ChainApi>>, ChainApi::Error> {
		log::debug!(target: LOG_TARGET, "[{:?}] view::submit_and_watch at:{}", self.pool.validated_pool().api().hash_and_length(&xt).0, self.at.hash);
		self.pool.submit_and_watch(&self.at, source, xt).await
	}

	/// Status of the pool associated withe the view.
	pub(super) fn status(&self) -> PoolStatus {
		self.pool.validated_pool().status()
	}

	/// Creates a watcher for given transaction.
	pub(super) fn create_watcher(
		&self,
		tx_hash: ExtrinsicHash<ChainApi>,
	) -> Watcher<ExtrinsicHash<ChainApi>, ExtrinsicHash<ChainApi>> {
		self.pool.validated_pool().create_watcher(tx_hash)
	}

	/// Revalidates some part of transaction from the internal pool.
	///
	/// Intended to run from revalidation worker. Revlidation can be terminated by sending message
	/// to the rx channel provided within `finish_revalidation_worker_channels`. Results are sent
	/// back over tx channels and shall be applied in maintain thread.
	pub(super) async fn revalidate_later(
		&self,
		finish_revalidation_worker_channels: FinishRevalidationWorkerChannels<ChainApi>,
	) {
		use sp_runtime::SaturatedConversion;

		let FinishRevalidationWorkerChannels {
			mut finish_revalidation_request_rx,
			revalidation_result_tx,
		} = finish_revalidation_worker_channels;

		log::debug!(target:LOG_TARGET, "view::revalidate_later: at {} starting", self.at.hash);
		let start = Instant::now();
		let validated_pool = self.pool.validated_pool();
		let api = validated_pool.api();

		let batch: Vec<_> = validated_pool.ready().map(|tx| tx.hash).collect();
		let batch_len = batch.len();

		//todo: sort batch by revalidation timestamp | maybe not needed at all? xts will be getting
		//out of the view...
		//todo: revalidate future, remove if invalid.

		let mut invalid_hashes = Vec::new();
		let mut revalidated = HashMap::new();

		let mut validation_results = vec![];
		let mut batch_iter = batch.into_iter();
		let mut should_break = false;
		loop {
			tokio::select! {
				_ = finish_revalidation_request_rx.recv() => {
					log::trace!(target: LOG_TARGET, "view::revalidate_later: finish revalidation request received at {}.", self.at.hash);
					should_break = true;
				}
				_ = async {
					if let Some(ext_hash) = batch_iter.next() {
						//todo clean up mess:
						if let Some(ext) = validated_pool.ready_by_hash(&ext_hash) {
							let validation_result = (api.validate_transaction(self.at.hash, ext.source, ext.data.clone()).await, ext_hash, ext);
							validation_results.push(validation_result);
						}
					} else {
						{
							self.revalidation_worker_channels.lock().as_mut().map(|v| v.remove_sender());
						}
						should_break = true;
					}
				} => {}
			}

			if should_break {
				break;
			}
		}

		log::info!(
			target:LOG_TARGET,
			"view::revalidate_later: at {:?} count: {}/{} took {:?}",
			self.at.hash,
			validation_results.len(),
			batch_len,
			start.elapsed()
		);
		log_xt_debug!(data:tuple, target:LOG_TARGET, validation_results.iter().map(|x| (x.1, &x.0)), "[{:?}] view::revalidate_later result: {:?}");

		for (validation_result, ext_hash, ext) in validation_results {
			match validation_result {
				Ok(Err(TransactionValidityError::Invalid(_))) => {
					invalid_hashes.push(ext_hash);
				},
				Ok(Err(TransactionValidityError::Unknown(_))) => {
					// skipping unknown, they might be pushed by valid or invalid transaction
					// when latter resubmitted.
				},
				Ok(Ok(validity)) => {
					revalidated.insert(
						ext_hash,
						ValidatedTransaction::valid_at(
							self.at.number.saturated_into::<u64>(),
							ext_hash,
							ext.source,
							ext.data.clone(),
							api.hash_and_length(&ext.data).1,
							validity,
						),
					);
				},
				Err(validation_err) => {
					log::trace!(
						target: LOG_TARGET,
						"[{:?}]: Removing due to error during revalidation: {}",
						ext_hash,
						validation_err
					);
					invalid_hashes.push(ext_hash);
				},
			}
		}

		log::debug!(target:LOG_TARGET, "view::revalidate_later: sending revalidation result at {}", self.at.hash);
		if let Err(e) = revalidation_result_tx
			.send(RevalidationResult { invalid_hashes, revalidated })
			.await
		{
			log::debug!(target:LOG_TARGET, "view::revalidate_later: sending revalidation_result at {} failed {:?}", self.at.hash, e);
		}
	}

	/// Sends revalidation request to the backround worker.
	///
	/// Also creates communication channels.
	/// Intended to ba called from maintain thread.
	pub(super) async fn start_background_revalidation(
		view: Arc<Self>,
		revalidation_queue: Arc<
			super::view_revalidation::RevalidationQueue<ChainApi, ChainApi::Block>,
		>,
	) {
		log::trace!(target:LOG_TARGET,"view::start_background_revalidation: at {}", view.at.hash);
		let (finish_revalidation_request_tx, finish_revalidation_request_rx) =
			tokio::sync::mpsc::channel(1);
		let (revalidation_result_tx, revalidation_result_rx) = tokio::sync::mpsc::channel(1);

		let finish_revalidation_worker_channels = FinishRevalidationWorkerChannels::new(
			finish_revalidation_request_rx,
			revalidation_result_tx,
		);

		let finish_revalidation_local_channels = FinishRevalidationLocalChannels::new(
			finish_revalidation_request_tx,
			revalidation_result_rx,
		);

		*view.revalidation_worker_channels.lock() = Some(finish_revalidation_local_channels);
		revalidation_queue
			.revalidate_later(view.clone(), finish_revalidation_worker_channels)
			.await;
	}

	/// Terminates background revalidation.
	///
	/// Receives the results from the worker and applies them to the internal pool.
	/// Intended to ba called from maintain thread.
	pub(super) async fn finish_revalidation(&self) {
		log::trace!(target:LOG_TARGET,"view::finish_revalidation: at {}", self.at.hash);
		let Some(revalidation_worker_channels) = self.revalidation_worker_channels.lock().take()
		else {
			log::trace!(target:LOG_TARGET, "view::finish_revalidation: no finish_revalidation_request_tx");
			return
		};

		let FinishRevalidationLocalChannels {
			finish_revalidation_request_tx,
			mut revalidation_result_rx,
		} = revalidation_worker_channels;

		if let Some(finish_revalidation_request_tx) = finish_revalidation_request_tx {
			if let Err(e) = finish_revalidation_request_tx.send(()).await {
				log::trace!(target:LOG_TARGET, "view::finish_revalidation: sending cancellation request at {} failed {:?}", self.at.hash, e);
			}
		}

		if let Some(revalidation_result) = revalidation_result_rx.recv().await {
			let start = Instant::now();
			let revalidated_len = revalidation_result.revalidated.len();
			let validated_pool = self.pool.validated_pool();
			validated_pool.remove_invalid(&revalidation_result.invalid_hashes);
			if revalidation_result.revalidated.len() > 0 {
				self.pool.resubmit(revalidation_result.revalidated);
			}
			log::info!(
				target:LOG_TARGET,
				"view::finish_revalidation: applying revalidation result invalid: {} revalidated: {} at {:?} took {:?}",
				revalidation_result.invalid_hashes.len(),
				revalidated_len,
				self.at.hash,
				start.elapsed()
			);
		}
	}
}
