#![feature(align_offset)]
use std::{
    cell::RefCell,
    cmp::{self, max},
    iter, mem, ptr, slice,
};

/// Initial size in bytes.
const INITIAL_SIZE: usize = 1024;
/// Minimum capacity. Must be larger than 0.
const MIN_CAPACITY: usize = 1;

struct ChunkList {
    w_ptr: *mut u8,
    end_ptr: *mut u8,
    current: Vec<u8>,
    rest: Vec<Vec<u8>>,
}

impl ChunkList {
    #[inline(never)]
    #[cold]
    fn reserve(&mut self, additional: usize) {
        let double_cap = self
            .current
            .capacity()
            .checked_mul(2)
            .expect("capacity overflow");
        let required_cap = additional
            .checked_next_power_of_two()
            .expect("capacity overflow");
        let new_capacity = cmp::max(double_cap, required_cap);
        let chunk = mem::replace(&mut self.current, Vec::with_capacity(new_capacity));
        self.rest.push(chunk);
        self.w_ptr = self.current.as_mut_ptr();
        self.end_ptr = unsafe { self.w_ptr.add(additional) };
    }

    fn write_ptr<T:Copy>(&mut self) -> *mut u8 {
        unsafe { self.w_ptr.add(self.w_ptr.align_offset(mem::align_of::<T>())) }
    }

    fn set_write_ptr(&mut self, w_ptr: *mut u8) {
        assert!(w_ptr <= self.end_ptr);
        self.w_ptr = w_ptr;
        // don't care about vector length
    }

    fn end_ptr(&mut self) -> *mut u8 {
        unsafe { self.current.as_mut_ptr().add(self.current.capacity()) }
    }

    fn new(cap: usize) -> ChunkList {
        let mut current = Vec::with_capacity(cap);
        let rest = Vec::new();
        let w_ptr: *mut u8 = current.as_mut_ptr();
        let end_ptr = unsafe { w_ptr.add(cap) };
        ChunkList {
            current,
            rest,
            w_ptr,
            end_ptr
        }
    }
}

/// An arena for Copy types (which don't have any destructors, hence the name).
pub struct DroplessArena {
    chunks: RefCell<ChunkList>,
}

impl DroplessArena {
    /// Creates a new arena with an unspecified default initial capacity.
    pub fn new() -> DroplessArena {
        DroplessArena::with_capacity(INITIAL_SIZE)
    }

    /// Creates a new arena with a specified initial capacity.
    pub fn with_capacity(n: usize) -> DroplessArena {
        let n = cmp::max(MIN_CAPACITY, n);
        DroplessArena {
            chunks: RefCell::new(ChunkList::new(n)),
        }
    }

    /// Stores a value of type `T` into the arena.
    ///
    /// Returns a mutable reference to the stored values.
    pub fn alloc<T: Copy>(&self, value: T) -> &mut T {
        &mut self.alloc_extend(iter::once(value))[0]
    }

    /// Allocates uninitialized space for `len` elements of type `T`.
    pub unsafe fn alloc_uninitialized<T: Copy>(&self, len: usize) -> &mut [T] {
        // may be possible with artificially created slices
        assert!(len*mem::size_of::<T>() < isize::max_value() as usize);
        let mut chunks = self.chunks.try_borrow_mut().expect("reentrant call to `DroplessArena::alloc_extend`");
        let mut w_ptr = chunks.write_ptr::<T>() as *mut T;
        let end = chunks.end_ptr();

        if w_ptr.add(len) as *mut u8 > end {
            // not enough free space, create new chunk
            chunks.reserve(len * mem::size_of::<T>());
            w_ptr = chunks.write_ptr::<T>() as *mut T;
        }

        // reserve space
        let start_ptr = w_ptr;
        w_ptr = w_ptr.add(len);
        chunks.set_write_ptr(w_ptr as *mut u8);

        let new_slice = {
            let new_slice = slice::from_raw_parts_mut(start_ptr, len);
            mem::transmute::<&mut [T], &mut [T]>(new_slice)
        };

        new_slice
    }

    /// Stores all elements from the provided iterator into a contiguous slice inside the arena.
    ///
    /// Returns a mutable slice to the stored values.
    ///
    /// # Note
    /// This method uses `Iterator::size_hint` to preallocate space in the arena.
    /// This is more efficient if the exact number of elements returned by the iterator is known in
    /// advance.
    pub fn alloc_extend<I, T: Copy>(&self, iterable: I) -> &mut [T]
        where
            I: IntoIterator<Item = T>,
    {
        let mut iter = iterable.into_iter();
        let mut chunks = self.chunks.try_borrow_mut().expect("reentrant call to DroplessArena::alloc_extend");

        let iter_min_len = iter.size_hint().0;

        let mut i = 0;
        let mut start_ptr = chunks.write_ptr::<T>() as *mut T;
        let mut w_ptr = start_ptr;

        unsafe {
            while let Some(elem) = iter.next() {
                let end_ptr = chunks.end_ptr();
                if w_ptr.add(max(1, iter_min_len)) as *mut u8 > end_ptr
                {
                    // The iterator was larger than we could fit into the current chunk.
                    let chunks = &mut *chunks;
                    // Create a new chunk into which we can freely push the entire iterator into
                    // i + 1 for the next one, and * 2 to be sure (and have enough alignment space)
                    let new_len = max((i + 1) * 2, iter_min_len);
                    chunks.reserve(mem::size_of::<T>() * new_len);
                    // copy data from previous chunk
                    let dst = chunks.write_ptr::<T>() as *mut T;
                    let src = start_ptr;
                    ptr::copy_nonoverlapping(src, dst, i);
                    w_ptr = dst.add(i);
                    start_ptr = dst;
                }

                // insert element (enough size now)
                w_ptr.write(elem);
                w_ptr = w_ptr.add(1);
                i += 1;
            }
        }

        chunks.set_write_ptr(w_ptr as *mut u8);

        let new_slice = unsafe {
            let new_slice = slice::from_raw_parts_mut(start_ptr, i);
            mem::transmute::<&mut [T], &mut [T]>(new_slice)
        };

        new_slice
    }
}

#[cfg(test)]
mod tests
{
    #[test]
    fn compiles() {
    }
}