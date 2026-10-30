// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Cumulus.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Tools for reclaiming PoV weight in parachain runtimes.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
use sp_externalities::{Externalities, ExternalitiesExt};

use sp_runtime_interface::runtime_interface;

#[cfg(feature = "std")]
use sp_trie::proof_size_extension::ProofSizeExt;

pub const PROOF_RECORDING_DISABLED: u64 = u64::MAX;

/// Body of version 1 of [`storage_proof_size::storage_proof_size`]: the size of the proof
/// recorded so far, or [`PROOF_RECORDING_DISABLED`] without [`ProofSizeExt`].
///
/// Trie nodes that the storage root at the end of the block will read for the writes of the
/// block are not included; version 2 ([`storage_proof_size_v2`]) estimates them. Runtimes built
/// against version 1 call it forever, and weight reclaim writes its result into the state, so
/// this body must stay as it is: blocks in the history of a chain must import bit-for-bit, and
/// collator and PVF must agree for a given runtime.
///
/// It is a free function, not a method of `sp_externalities::Externalities` like the bodies of
/// the versioned storage host functions, because the size comes from the [`ProofSizeExt`]
/// extension: `sp-trie` defines it on top of `sp-externalities`, so the trait cannot name it,
/// and the host function belongs to cumulus, not to substrate's `sp-io`. Being public, it can be
/// called directly, which the bare host function call cannot do for a specific version: the
/// runtime interface macro keeps the per-version functions private.
///
/// Test: the version 1 compatibility suite of `cumulus-proof-size-hostfunction-fuzz`
/// (`tests/v1_compat.rs`) compares this body with master; it reads [`ProofSizeExt`] itself, the
/// same way, because master has no such function.
#[cfg(feature = "std")]
pub fn storage_proof_size_v1(mut ext: &mut dyn Externalities) -> u64 {
	ext.extension::<ProofSizeExt>()
		.map_or(PROOF_RECORDING_DISABLED, |e| e.storage_proof_size())
}

/// Body of version 2 of [`storage_proof_size::storage_proof_size`]: the size of the proof
/// recorded so far plus the trie nodes the storage root at the end of the block will read for
/// the writes so far, or [`PROOF_RECORDING_DISABLED`] without [`ProofSizeExt`].
///
/// It first records those trie nodes (`Externalities::record_proof_for_dirty_keys`), then reads
/// the size as [`storage_proof_size_v1`] does. Version 1 cannot be changed to do this, because
/// runtimes built against it call it forever and weight reclaim writes its result into the
/// state, so blocks in the history of a chain would no longer import.
///
/// Version 2 is registered with `register_only`: nodes provide it, but runtimes keep calling
/// version 1 until every node serves version 2. Dropping the marker must happen in the same
/// release as dropping it on `sp_io::storage::append` version 2 and on
/// `sp_io::default_child_storage::root` version 3, and as switching the three matching shims in
/// `cumulus-pallet-parachain-system`'s `validate_block` (`host_storage_append`,
/// `host_default_child_storage_root`, `host_storage_proof_size`) to the new bodies. The
/// estimation is only sound with the child root semantics of
/// `default_child_storage::root` version 3: with version 2, which writes child roots into the
/// main overlay, the fuzzer's pinned scripts `fuzz_child_emptied_root_computed_then_refilled`,
/// `fuzz_dropped_extrinsic_discards_empty_child_changes` and
/// `fuzz_dropped_extrinsic_clears_root_cache` estimate below the real proof size or make
/// collator and validator disagree.
///
/// It is a free function for the same reason as [`storage_proof_size_v1`].
///
/// Test: the estimation runner of `cumulus-proof-size-hostfunction-fuzz` (`tests/smoke.rs`)
/// calls this body, together with the new bodies of the two storage host functions, and checks
/// that the estimation is never exceeded and that collator and validator agree.
#[cfg(feature = "std")]
pub fn storage_proof_size_v2(mut ext: &mut dyn Externalities) -> u64 {
	if ext.extension::<ProofSizeExt>().is_some() {
		ext.record_proof_for_dirty_keys();
	}
	storage_proof_size_v1(ext)
}

/// Interface that provides access to the current storage proof size.
///
/// Should return the current storage proof size if [`ProofSizeExt`] is registered. Otherwise, needs
/// to return u64::MAX.
#[runtime_interface]
pub trait StorageProofSize {
	/// Returns the current storage proof size.
	fn storage_proof_size(&mut self) -> u64 {
		storage_proof_size_v1(*self)
	}

	/// Returns the current storage proof size after computing PoV size for storage root.
	///
	/// This function first triggers the computation of PoV size for storage root changes
	/// (recording trie nodes that would be accessed during storage root calculation),
	/// then returns the updated proof size.
	///
	/// Registered only, so runtimes keep calling version 1 until all nodes provide version 2.
	/// Dropping `register_only` must go together with enabling the dirty-key estimation in
	/// `host_storage_proof_size` of `cumulus-pallet-parachain-system`'s `validate_block`,
	/// otherwise collator and validator report different proof sizes, and with dropping
	/// `register_only` on `sp_io::storage::append` version 2 and
	/// `sp_io::default_child_storage::root` version 3 (and switching their shims), because the
	/// estimation is only sound with their semantics. See
	/// [`storage_proof_size_v2`](crate::storage_proof_size_v2).
	#[version(2, register_only)]
	fn storage_proof_size(&mut self) -> u64 {
		storage_proof_size_v2(*self)
	}
}

#[cfg(test)]
mod tests {
	use sp_core::Blake2Hasher;
	use sp_state_machine::TestExternalities;
	use sp_trie::{
		proof_size_extension::ProofSizeExt, recorder::Recorder, LayoutV1, PrefixedMemoryDB,
		TrieDBMutBuilder, TrieMut,
	};

	use crate::{storage_proof_size, PROOF_RECORDING_DISABLED};

	const TEST_DATA: &[(&[u8], &[u8])] = &[(b"key1", &[1; 64]), (b"key2", &[2; 64])];

	type TestLayout = LayoutV1<sp_core::Blake2Hasher>;

	fn get_prepared_test_externalities() -> (TestExternalities<Blake2Hasher>, Recorder<Blake2Hasher>)
	{
		let mut db = PrefixedMemoryDB::default();
		let mut root = Default::default();

		{
			let mut trie = TrieDBMutBuilder::<TestLayout>::new(&mut db, &mut root).build();
			for (k, v) in TEST_DATA {
				trie.insert(k, v).expect("Inserts data");
			}
		}

		let recorder: sp_trie::recorder::Recorder<Blake2Hasher> = Default::default();
		let trie_backend = sp_state_machine::TrieBackendBuilder::new(db, root)
			.with_recorder(recorder.clone())
			.build();

		let mut ext: TestExternalities<Blake2Hasher> = TestExternalities::default();
		ext.backend = trie_backend;
		(ext, recorder)
	}

	#[test]
	fn host_function_returns_size_from_recorder() {
		let (mut ext, recorder) = get_prepared_test_externalities();
		ext.register_extension(ProofSizeExt::new(recorder));

		ext.execute_with(|| {
			assert_eq!(storage_proof_size::storage_proof_size(), 0);
			sp_io::storage::get(b"key1");
			assert_eq!(storage_proof_size::storage_proof_size(), 175);
			sp_io::storage::get(b"key2");
			assert_eq!(storage_proof_size::storage_proof_size(), 275);
			sp_io::storage::get(b"key2");
			assert_eq!(storage_proof_size::storage_proof_size(), 275);
		});
	}

	#[test]
	fn host_function_does_not_estimate_dirty_keys() {
		let (mut ext, recorder) = get_prepared_test_externalities();
		ext.register_extension(ProofSizeExt::new(recorder.clone()));

		ext.execute_with(|| {
			sp_io::storage::set(b"key1", &[3; 64]);
			sp_io::storage::clear(b"key2");
			let recorded = recorder.estimate_encoded_size() as u64;
			assert_eq!(storage_proof_size::storage_proof_size(), recorded);

			sp_externalities::with_externalities(|ext| ext.record_proof_for_dirty_keys());
			assert!(storage_proof_size::storage_proof_size() > recorded);
		});
	}

	#[test]
	fn host_function_returns_max_without_extension() {
		let (mut ext, _) = get_prepared_test_externalities();

		ext.execute_with(|| {
			assert_eq!(storage_proof_size::storage_proof_size(), PROOF_RECORDING_DISABLED);
			sp_io::storage::get(b"key1");
			assert_eq!(storage_proof_size::storage_proof_size(), PROOF_RECORDING_DISABLED);
			sp_io::storage::get(b"key2");
			assert_eq!(storage_proof_size::storage_proof_size(), PROOF_RECORDING_DISABLED);
		});
	}
}
