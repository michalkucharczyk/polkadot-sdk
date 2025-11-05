# Incremental Storage Root Estimation for PoV Reclaim

## The Problem

Storage weight reclaim allows parachains to return unused PoV space from completed extrinsics, enabling subsequent transactions to use this space. However, a critical issue was discovered: **storage reclaim doesn't account for storage root calculation**.

When items are added or deleted, they're only noted in the overlay—no trie access occurs. But when calculating the storage root at block finalization, these items are looked up in the trie to add/delete/merge nodes. This causes the actual PoV size to significantly exceed estimates, potentially allowing blocks to overshoot the PoV budget.

## The Solution Concept

Naive appraoch of computing storage root estimation after every extrinsic killed performance (2% of original throuhtput). More details on the whole optimization story is summed up [here](https://hackmd.io/o_Ghc86OT4KzCE4x04MeOg?view).

This work introduces a concept of  **incremental storage root estimation** that approximates the trie node accesses that would occur during storage root calculation, without actually computing the root hash. This is triggered after each extrinsic via the weight reclaim pallet.

## High level overview:

### 1. Runtime Layer (Entry Point)
- `StorageWeightReclaim` transaction extension calls `trigger_storage_root_size_estimation()` in `post_dispatch_details`,
- This occurs after each extrinsic execution but before measuring final proof size,
- Allows the PoV size estimate to account for trie node accesses that would be made during storage root computation

### 2. State Machine Layer (snapshot mechanism)
- New `Changeset` tracking system maintains which keys have been modified,
- Incremental snapshots only return keys modified since the last snapshot, not all keys modified since block start,
- Supports nested storage transactions (commit/rollback),
- Deduplication strategy: Each snapshot captures "dirty keys" and moves them to a "captured" set, preventing reprocessing,

### 3. Trie Backend Layer (recording nodes without root computation)
- `trigger_storage_root_size_estimation()` performs trie node accesses for delta keys,
- Uses ephemeral trie overlay to access nodes WITHOUT computing the actual storage root hash,
- **Crucial assumption**: The read/delete operations on the ephemeral trie mirror exactly what storage root calculation would do - accessing nodes, triggering reorganizations, merging/splitting nodes. This ensures the trie nodes accessed during estimation are precisely the same nodes that would be accessed when computing the real storage root, providing accurate PoV size estimation.
- Records accessed nodes in the proof recorder, updating PoV size,
- Returns statistics (nodes accessed, proof size increase, execution time) - used in overhead estimation,

## Upstream Trie Changes

The trie repository [PR](https://github.com/paritytech/trie/pull/226) adds explicit commit-on-drop semantics to `TrieDBMut`. This ensures that ephemeral tries used for estimation are dropped without triggering storage root computation. Since storage root computation is a heavy operation, avoiding it on drop saves significant time.

## Validation Tests

Tests added (inspired by [this](https://github.com/paritytech/polkadot-sdk/pull/6230/files#diff-aee4478254ac14c2c059e9e1a0eb5b2d3694872bfdce872416319959223de977R1115)) in this [commit](https://github.com/michalkucharczyk/polkadot-sdk/commit/e3de902a42a46bfb64f8683d34be8121321bf1cf) validate the accuracy of the solution:

1. **Deterministic test** (`calculating_storage_root_should_not_change_storage_proof`):
   - Large storage state (100k keys + prefixed keys, both main and child storage)
   - Performs insertions and deletions
   - Validates that proof size after `trigger_storage_root_size_estimation()` equals proof size after actual `storage_root()`

2. **Randomized test** (`calculating_storage_root_should_not_change_storage_proof_random`):
   - Very large storage state (1M keys + prefixed keys)
   - 1,000 iterations with up to 10k random insertions/deletions per iteration
   - Validates proof size accuracy across diverse random workloads

These tests prove that the estimation triggers the exact same trie node accesses as actual storage root calculation, with no missing nodes or side effects.

Tests could be extended with fuzzing.

## Flow recap

1. Runtime executes extrinsic → storage changes (writes/removals) tracked in overlay's Changeset
2. Extrinsic completes → `trigger_storage_root_size_estimation()` called
3. Snapshot created → returns ONLY keys modified since previous snapshot
4. Trie estimation → reads/deletes trie nodes for delta keys (mirroring storage root computation), updates proof recorder
5. PoV size adjusted → now includes estimated trie node accesses
6. Process repeats for each extrinsic
7. Block finalization → full `storage_root()` computed once with all changes

## Performance Impact

From benchmarking results:
- **Naive approach** (computing full root per extrinsic): ~2% of original throughput
- **With all optimizations**: 2-7% performance drop in block production benchmarks for balance transfers. The impact to other extrinsics is not yet estimated.
- The key insight: incremental snapshots + deduplication + not computing actual hash = acceptable(?) overhead

## Future Work

Better understanding of performance impact is needed. This is work in progress.

The current implementation uses a dedicated `trigger_storage_root_size_estimation()` function. This should be merged with the existing `pov_size` host function, which will require:
- A new RFC to define the updated interface
- Adding a parameter to `pov_size` to enable the trigger behavior
- This consolidation will provide a cleaner API surface
