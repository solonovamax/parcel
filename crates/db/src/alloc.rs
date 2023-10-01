use std::{cell::UnsafeCell, marker::PhantomData, ptr::NonNull};

use allocator_api2::alloc::{AllocError, Allocator, Layout};

use crate::atomics::AtomicVec;

const PAGE_SIZE: usize = 65536;
const PTR_MAX: u32 = u32::MAX;
const NUM_PAGES: u32 = PTR_MAX / (PAGE_SIZE as u32) + 1;
const PAGE_INDEX_SIZE: u32 = NUM_PAGES.ilog2();
const PAGE_INDEX_SHIFT: u32 = 32 - PAGE_INDEX_SIZE;
const PAGE_INDEX_MASK: u32 = ((1 << PAGE_INDEX_SIZE) - 1) << PAGE_INDEX_SHIFT;
const PAGE_OFFSET_MASK: u32 = (1 << PAGE_INDEX_SHIFT) - 1;

#[inline]
fn unpack_addr(addr: u32) -> (u32, u32) {
  let page_index = (addr & PAGE_INDEX_MASK) >> PAGE_INDEX_SHIFT;
  let offset = addr & PAGE_OFFSET_MASK;
  (page_index, offset)
}

#[inline]
fn pack_addr(page: u32, offset: u32) -> u32 {
  (page << PAGE_INDEX_SHIFT) | (offset & PAGE_OFFSET_MASK)
}

pub struct PageAllocator {
  pages: AtomicVec<Page>,
}

unsafe impl Send for PageAllocator {}

struct Page {
  ptr: *mut u8,
  len: usize,
}

impl Drop for Page {
  fn drop(&mut self) {
    println!("DROP PAGE");
    let layout = unsafe { Layout::from_size_align_unchecked(self.len, 8) };
    unsafe { std::alloc::dealloc(self.ptr.cast(), layout) };
  }
}

impl PageAllocator {
  pub const fn new() -> Self {
    Self {
      pages: AtomicVec::new(),
    }
  }

  unsafe fn alloc_page(&self, min_size: usize, zeroed: bool) -> u32 {
    let len = min_size.max(PAGE_SIZE);
    let layout = Layout::from_size_align_unchecked(len, 8);

    let ptr = if zeroed {
      std::alloc::alloc_zeroed(layout)
    } else {
      std::alloc::alloc(layout)
    };

    // println!("ALLOC PAGE {:?}", self.pages.len());
    self.pages.push(Page { ptr, len })
  }

  pub unsafe fn get<T>(&self, addr: u32) -> *mut T {
    let (page_index, offset) = unpack_addr(addr);
    let ptr = self
      .pages
      .get_unchecked(page_index)
      .ptr
      .add(offset as usize);
    ptr as *mut T
  }

  pub unsafe fn get_slice(&self, addr: u32, len: usize) -> &mut [u8] {
    let ptr: *mut u8 = self.get(addr);
    core::slice::from_raw_parts_mut(ptr, len)
  }

  pub unsafe fn get_page(&self, index: u32) -> &mut [u8] {
    let page = &self.pages.get_unchecked(index);
    core::slice::from_raw_parts_mut(page.ptr, page.len)
  }

  pub unsafe fn find_page(&self, ptr: *const u8) -> Option<u32> {
    for i in 0..self.pages.len() {
      let page = self.get_page(i);
      if page.as_ptr_range().contains(&ptr) {
        return Some(pack_addr(i, (ptr as usize - page.as_ptr() as usize) as u32));
      }
    }

    None
  }

  pub fn dump(&self) {
    for i in 0..self.pages.len() {
      let page = unsafe { self.get_page(i) };
      std::fs::write(format!(".parcel-cache/page.{}.bin", i), page).unwrap();
    }
  }
}

unsafe impl Allocator for PageAllocator {
  #[inline(always)]
  fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    unsafe {
      let page_index = self.alloc_page(layout.size(), false);
      let page = self.get_page(page_index);
      Ok(NonNull::new_unchecked(page))
    }
  }

  #[inline(always)]
  fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
    unsafe {
      let page_index = self.alloc_page(layout.size(), true);
      let page = self.get_page(page_index);
      Ok(NonNull::new_unchecked(page))
    }
  }

  unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
    println!("DEALLOC PAGE {:p}", ptr);
  }
}

pub struct Arena {
  addr: UnsafeCell<u32>,
}

impl Default for Arena {
  fn default() -> Self {
    Arena::new()
  }
}

impl Arena {
  pub const fn new() -> Self {
    Self {
      addr: UnsafeCell::new(1),
    }
  }

  pub fn alloc(&self, size: u32) -> u32 {
    let size = (size + 7) & !7;
    unsafe {
      let ptr = self.addr.get();
      let addr = *ptr;
      if addr == 1 {
        let page_index = current_heap().alloc_page(size as usize, false);
        *ptr = pack_addr(page_index, size);
        return pack_addr(page_index, 0);
      }

      let (page_index, offset) = unpack_addr(addr);
      let page = current_heap().get_page(page_index);
      if (offset + size) as usize >= page.len() {
        let page_index = current_heap().alloc_page(size as usize, false);
        *ptr = pack_addr(page_index, size);
        pack_addr(page_index, 0)
      } else {
        *ptr += size;
        addr
      }
    }
  }

  pub unsafe fn dealloc(&self, ptr: NonNull<u8>, layout: Layout) {
    let addr_ptr = self.addr.get();
    let addr = *addr_ptr;
    debug_assert!(addr != 1);

    let (page_index, offset) = unpack_addr(addr);
    if offset == 0 {
      return;
    }

    let page = current_heap().get_page(page_index);
    let cur_ptr = (page.as_ptr() as usize) + offset as usize;
    let end_ptr = (ptr.as_ptr() as usize) + ((layout.size() + 7) & !7);
    if cur_ptr == end_ptr {
      println!("DEALLOC AT END");
      *addr_ptr -= layout.size() as u32;
    }
  }
}

// pub struct ArenaAllocator;

// // static WASTED: AtomicUsize = AtomicUsize::new(0);

// unsafe impl Allocator for ArenaAllocator {
//   #[inline(always)]
//   fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
//     let addr = ARENA.alloc(layout.size() as u32);
//     unsafe {
//       Ok(NonNull::new_unchecked(
//         ARENA.alloc.get_slice(addr, layout.size()),
//       ))
//     }
//   }

//   // #[inline(always)]
//   // fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
//   //   unsafe { Ok(self.alloc_page(layout.size(), true)) }
//   // }

//   unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
//     // use std::backtrace::Backtrace;
//     // WASTED.fetch_add(layout.size(), std::sync::atomic::Ordering::SeqCst);
//     // println!(
//     //   "DEALLOC ARENA {:p} {} {:?} {}",
//     //   ptr,
//     //   layout.size(),
//     //   WASTED,
//     //   Backtrace::force_capture()
//     // );
//     // ARENA.dealloc(ptr, layout)
//   }
// }

pub struct Slab<T> {
  free_head: u32,
  phantom: PhantomData<T>,
}

impl<T> Default for Slab<T> {
  fn default() -> Self {
    Slab::new()
  }
}

#[derive(Debug)]
struct FreeNode {
  slots: u32,
  next: u32,
}

impl<T> Slab<T> {
  pub const fn new() -> Self {
    Slab {
      free_head: 1,
      phantom: PhantomData,
    }
  }

  pub fn alloc(&mut self, count: u32) -> u32 {
    unsafe {
      let size = std::mem::size_of::<T>() as u32;
      if self.free_head != 1 {
        let mut addr = self.free_head;
        let mut prev: *mut u32 = &mut self.free_head;
        loop {
          let node = &mut *current_heap().get::<FreeNode>(addr);
          if node.slots >= count {
            if count < node.slots {
              node.slots -= count;
              addr += size * node.slots;
            } else {
              *prev = node.next;
            }
            // println!(
            //   "REUSED {:?} {} {} {:?}",
            //   unpack_addr(addr),
            //   count,
            //   node.slots,
            //   unpack_addr(node.next)
            // );
            // self.debug_free_list();
            return addr;
          }
          if node.next == 1 {
            break;
          }
          prev = &mut node.next;
          addr = node.next;
        }
      }

      current_arena().alloc(size * count)
    }
  }

  pub fn dealloc(&mut self, addr: u32, mut count: u32) {
    // println!("DEALLOC {} {}", addr, count);
    unsafe {
      // let size = std::mem::size_of::<T>() as u32;
      // if self.free_head != 1 {
      //   let node = &mut *HEAP.get::<FreeNode>(self.free_head);
      //   if addr + size * count == self.free_head {
      //     count += node.slots;
      //     self.free_head = node.next;
      //   } else if self.free_head + size * node.slots == addr {
      //     node.slots += count;
      //     return;
      //   }
      // }

      let node = &mut *current_heap().get::<FreeNode>(addr);
      node.slots = count;
      node.next = self.free_head;
      self.free_head = addr;
      // self.debug_free_list();
    }
  }

  fn debug_free_list(&self) {
    let mut addr = self.free_head;
    let mut free = 0;
    while addr != 1 {
      let node = unsafe { &*current_heap().get::<FreeNode>(addr) };
      println!("{} {:?}", addr, node);
      free += node.slots;
      addr = node.next;
    }
    println!("FREE SLOTS: {}", free);
  }
}

#[thread_local]
pub static mut HEAP: Option<&'static PageAllocator> = None;
#[thread_local]
pub static mut ARENA: Option<&'static Arena> = None;

pub fn current_heap<'a>() -> &'a PageAllocator {
  unsafe { HEAP.unwrap_unchecked() }
}

pub fn current_arena<'a>() -> &'a Arena {
  unsafe { ARENA.unwrap_unchecked() }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_slab() {
    struct Test {
      foo: u32,
      bar: u32,
    }

    let mut slab = Slab::<Test>::new();
    let addr1 = slab.alloc(5);
    assert_eq!(addr1, 0);
    let addr2 = slab.alloc(2);
    assert_eq!(addr2, 40);
    slab.dealloc(addr1, 5);
    let addr = slab.alloc(1);
    assert_eq!(addr, 32);
    slab.dealloc(addr2, 2);
    let addr = slab.alloc(4);
    assert_eq!(addr, 0);
    slab.debug_free_list();
    // let addr = slab.alloc(2);
    // assert_eq!(addr, 24);
    // let addr = slab.alloc(2);
    // assert_eq!(addr, 24);
  }
}
