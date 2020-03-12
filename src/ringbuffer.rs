//! # Transient RingBuffer implementation
//!
//! This module provides a trait and implementation for a ringbuffer that
//! abstracts over storage items and presents them as a FIFO queue.
//!
//! Usage Example:
//! ```rust,ignore
//! use ringbuffer::{RingBufferTrait, RingBufferTransient};
//!
//! // Trait object that we will be interacting with.
//! type RingBuffer = dyn RingBufferTrait<
//!     SomeStruct,
//!     Bounds = <TestModule as Store>::TestRange,
//!     Map = <TestModule as Store>::TestMap,
//! >;
//! // Implementation that we will instantiate.
//! type Transient = RingBufferTransient<
//!     SomeStruct,
//!     <TestModule as Store>::TestRange,
//!     <TestModule as Store>::TestMap,
//!     RingBuffer,
//! >;
//! {
//!     let mut ring: Box<RingBuffer> = Box::new(Transient::new());
//!     ring.push(SomeStruct { foo: 1, bar: 2 });
//! } // `ring.commit()` will be called on `drop` here and syncs to storage
//! ```
//!
//! Note: You might want to introduce a helper function that wraps the complex
//! types and just returns the boxed trait object.

use codec::{Codec, EncodeLike};
use core::marker::PhantomData;
use frame_support::storage::{StorageMap, StorageValue};

pub type Index = u16;

/// Transient backing data that is the backbone of the trait object.
pub struct RingBufferTransient<I, B, M, T>
where
	I: Codec + EncodeLike,
	B: StorageValue<(Index, Index), Query = (Index, Index)>,
	M: StorageMap<Index, I, Query = I>,
	T: RingBufferTrait<I, Bounds = B, Map = M> + ?Sized,
{
	start: Index,
	end: Index,
	_t: PhantomData<(I, B, M, T)>,
}

impl<I, B, M, T> RingBufferTransient<I, B, M, T>
where
	I: Codec + EncodeLike,
	B: StorageValue<(Index, Index), Query = (Index, Index)>,
	M: StorageMap<Index, I, Query = I>,
	T: RingBufferTrait<I, Bounds = B, Map = M> + ?Sized,
{
	/// Create a new RingBufferTransient that backs the ringbuffer implementation.
	///
	/// Initializes itself from the Bounds.
	pub fn new() -> RingBufferTransient<I, B, M, T> {
		let (start, end) = <<T as RingBufferTrait<I>>::Bounds>::get();
		RingBufferTransient {
			start,
			end,
			_t: PhantomData,
		}
	}
}

impl<I, B, M, T> Drop for RingBufferTransient<I, B, M, T>
where
	I: Codec + EncodeLike,
	B: StorageValue<(Index, Index), Query = (Index, Index)>,
	M: StorageMap<Index, I, Query = I>,
	T: RingBufferTrait<I, Bounds = B, Map = M> + ?Sized,
{
	fn drop(&mut self) {
		<Self as RingBufferTrait<I>>::commit(self);
	}
}

/// Trait object presenting the ringbuffer interface.
pub trait RingBufferTrait<I>
where
	I: Codec + EncodeLike,
{
	type Bounds: StorageValue<(Index, Index)>;
	type Map: StorageMap<Index, I>;

	/// Store all changes made in the underlying storage.
	///
	/// Data is not guaranteed to be consistent before this call.
	/// Will be called by `drop`.
	fn commit(&self);

	/// Push an item onto the end of the queue.
	fn push(&mut self, i: I);
	/// Push an item onto the front of the queue.
	fn push_front(&mut self, i: I);

	/// Pop an item from the start of the queue.
	///
	/// Returns `None` if the queue is empty.
	fn pop(&mut self) -> Option<I>;

	/// Return whether the queue is empty.
	fn is_empty(&self) -> bool;
}

/// Ringbuffer implementation based on `RingBufferTransient`
impl<I, B, M, T> RingBufferTrait<I> for RingBufferTransient<I, B, M, T>
where
	I: Codec + EncodeLike,
	B: StorageValue<(Index, Index), Query = (Index, Index)>,
	M: StorageMap<Index, I, Query = I>,
	T: RingBufferTrait<I, Bounds = B, Map = M> + ?Sized,
{
	type Bounds = B;
	type Map = M;

	/// Commit the (potentially) changed bounds to storage.
	fn commit(&self) {
		Self::Bounds::put((self.start, self.end));
	}

	/// Push an item onto the end of the queue.
	///
	/// Will insert the new item, but will not update the bounds in storage.
	fn push(&mut self, item: I) {
		Self::Map::insert(self.end, item);
		// this will intentionally overflow and wrap around when bonds_end
		// reaches `Index::max_value` because we want a ringbuffer.
		let next_index = self.end.wrapping_add(1);
		if next_index == self.start {
			// queue presents as empty but is not
			// --> overwrite the oldest item in the FIFO ringbuffer
			self.start = self.start.wrapping_add(1);
		}
		self.end = next_index;
	}

	/// Push an item onto the front of the queue.
	///
	/// Equivalent to `push` if the queue is empty.
	fn push_front(&mut self, item: I) {
		if self.is_empty() {
			// queue is empty --> regular push
			self.push(item);
			return;
		}
		let index = self.start.wrapping_sub(1);
		Self::Map::insert(index, item);
		self.start = index;
		if self.start == self.end {
			// queue presents as empty but is not
			// --> overwrite the most recent item in the queue
			self.end = self.end.wrapping_sub(1);
		}
	}

	/// Pop an item from the start of the queue.
	fn pop(&mut self) -> Option<I> {
		if self.is_empty() {
			return None;
		}
		let item = Self::Map::take(self.start);
		self.start = self.start.wrapping_add(1);

		item.into()
	}

	fn is_empty(&self) -> bool {
		self.start == self.end
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use RingBufferTrait;

	use codec::{Decode, Encode};
	use frame_support::{decl_module, decl_storage, impl_outer_origin, parameter_types, weights::Weight};
	use sp_core::H256;
	use sp_runtime::{
		testing::Header,
		traits::{BlakeTwo256, IdentityLookup},
		Perbill,
	};
	use system;

	impl_outer_origin! {
		pub enum Origin for Test {}
	}

	// For testing the pallet, we construct most of a mock runtime. This means
	// first constructing a configuration type (`Test`) which `impl`s each of the
	// configuration traits of modules we want to use.
	#[derive(Clone, Eq, PartialEq)]
	pub struct Test;

	pub trait Trait: system::Trait {}

	decl_module! {
		pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		}
	}

	#[derive(Clone, PartialEq, Encode, Decode, Default, Debug)]
	pub struct SomeStruct {
		foo: u64,
		bar: u64,
	}

	decl_storage! {
		trait Store for Module<T: Trait> as RingBufferTest {
			TestMap get(fn get_test_value): map hasher(twox_64_concat) Index => SomeStruct;
			TestRange get(fn get_test_range): (Index, Index) = (0, 0);
		}
	}

	parameter_types! {
		pub const BlockHashCount: u64 = 250;
		pub const MaximumBlockWeight: Weight = 1024;
		pub const MaximumBlockLength: u32 = 2 * 1024;
		pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
	}

	impl system::Trait for Test {
		type Origin = Origin;
		type Call = ();
		type Index = u64;
		type BlockNumber = u64;
		type Hash = H256;
		type Hashing = BlakeTwo256;
		type AccountId = u64;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Header = Header;
		type Event = ();
		type BlockHashCount = BlockHashCount;
		type MaximumBlockWeight = MaximumBlockWeight;
		type MaximumBlockLength = MaximumBlockLength;
		type AvailableBlockRatio = AvailableBlockRatio;
		type Version = ();
		type ModuleToIndex = ();
		type AccountData = ();
		type OnNewAccount = ();
		type OnKilledAccount = ();
	}

	impl Trait for Test {}

	type TestModule = Module<Test>;

	// This function basically just builds a genesis storage key/value store according to
	// our desired mockup.
	fn new_test_ext() -> sp_io::TestExternalities {
		let storage = system::GenesisConfig::default().build_storage::<Test>().unwrap();
		storage.into()
	}

	// ------------------------------------------------------------
	// ringbuffer

	// Trait object that we will be interacting with.
	type RingBuffer = dyn RingBufferTrait<
		SomeStruct,
		Bounds = <TestModule as Store>::TestRange,
		Map = <TestModule as Store>::TestMap,
	>;
	// Implementation that we will instantiate.
	type Transient = RingBufferTransient<
		SomeStruct,
		<TestModule as Store>::TestRange,
		<TestModule as Store>::TestMap,
		RingBuffer,
	>;

	#[test]
	fn simple_push() {
		new_test_ext().execute_with(|| {
			let mut ring: Box<RingBuffer> = Box::new(Transient::new());
			ring.push(SomeStruct { foo: 1, bar: 2 });
			ring.commit();
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (0, 1));
			let some_struct = TestModule::get_test_value(0);
			assert_eq!(some_struct, SomeStruct { foo: 1, bar: 2 });
		})
	}

	#[test]
	fn drop_does_commit() {
		new_test_ext().execute_with(|| {
			{
				let mut ring: Box<RingBuffer> = Box::new(Transient::new());
				ring.push(SomeStruct { foo: 1, bar: 2 });
			}
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (0, 1));
			let some_struct = TestModule::get_test_value(0);
			assert_eq!(some_struct, SomeStruct { foo: 1, bar: 2 });
		})
	}

	#[test]
	fn simple_pop() {
		new_test_ext().execute_with(|| {
			let mut ring: Box<RingBuffer> = Box::new(Transient::new());
			ring.push(SomeStruct { foo: 1, bar: 2 });

			let item = ring.pop();
			ring.commit();
			assert!(item.is_some());
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (1, 1));
		})
	}

	#[test]
	fn overflow_wrap_around() {
		new_test_ext().execute_with(|| {
			let mut ring: Box<RingBuffer> = Box::new(Transient::new());

			for i in 1..(u16::max_value() as u64) + 2 {
				ring.push(SomeStruct { foo: 42, bar: i });
			}
			ring.commit();
			let start_end = TestModule::get_test_range();
			assert_eq!(
				start_end,
				(1, 0),
				"range should be inverted because the index wrapped around"
			);

			let item = ring.pop();
			ring.commit();
			let (start, end) = TestModule::get_test_range();
			assert_eq!(start..end, 2..0);
			let item = item.expect("an item should be returned");
			assert_eq!(
				item.bar, 2,
				"the struct for field `bar = 2`, was placed at index 1"
			);

			let item = ring.pop();
			ring.commit();
			let (start, end) = TestModule::get_test_range();
			assert_eq!(start..end, 3..0);
			let item = item.expect("an item should be returned");
			assert_eq!(
				item.bar, 3,
				"the struct for field `bar = 3`, was placed at index 2"
			);

			for i in 1..4 {
				ring.push(SomeStruct { foo: 21, bar: i });
			}
			ring.commit();
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (4, 3));

			// push_front should overwrite the most recent entry if the queue is full
			ring.push_front(SomeStruct { foo: 4, bar: 4});
			ring.commit();
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (3, 2));
		})
	}

	#[test]
	fn simple_push_front() {
		new_test_ext().execute_with(|| {
			let mut ring: Box<RingBuffer> = Box::new(Transient::new());
			ring.push_front(SomeStruct { foo: 1, bar: 2 });
			ring.commit();
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (0, 1));

			ring.push_front(SomeStruct { foo: 20, bar: 42 });
			ring.commit();
			let start_end = TestModule::get_test_range();
			assert_eq!(start_end, (u16::max_value(), 1));
		})
	}
}
