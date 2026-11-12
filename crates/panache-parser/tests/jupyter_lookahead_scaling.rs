use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use panache_parser::{Flavor, ParserOptions, parse};

struct CountingAllocator;

thread_local! {
    static ALLOCATED_BYTES: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation(size: usize) {
    let _ = ALLOCATED_BYTES.try_with(|bytes| {
        if let Some(total) = bytes.get() {
            bytes.set(Some(total + size));
        }
    });
}

// SAFETY: Every operation delegates to System with the original pointer and layout.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: The caller supplies the layout required by GlobalAlloc.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: The caller supplies the layout required by GlobalAlloc.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation(new_size);
        // SAFETY: The caller supplies System's allocation and its original layout.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The caller supplies System's allocation and its original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn parse_allocations(input: &str) -> usize {
    let options = ParserOptions::for_flavor(Flavor::Jupyter);
    ALLOCATED_BYTES.with(|bytes| bytes.set(Some(0)));
    let tree = parse(input, Some(options));
    let allocated = ALLOCATED_BYTES.with(|bytes| bytes.replace(None).unwrap());
    assert_eq!(tree.text().to_string(), input);
    allocated
}

fn assert_linear_allocations(block: &str, count: usize) {
    let input = block.repeat(count);
    // Warm the parser's thread-local scratch buffers before measuring either size.
    parse_allocations(&input);
    let small = parse_allocations(&input);
    let large = parse_allocations(&input.repeat(2));
    assert!(
        large < small * 3,
        "doubling the input should stay below 3x allocated bytes: {small} -> {large}"
    );
}

#[test]
fn ordinary_tables_separated_by_headings_allocate_linearly() {
    assert_linear_allocations("| A | B |\n|---|---|\n| x | y |\n# Heading\n", 1024);
}

#[test]
fn multiline_math_tables_separated_by_headings_allocate_linearly() {
    assert_linear_allocations("| A | B |\n|---|---|\n| $x\n+y$ | z |\n# Heading\n", 1024);
}

#[test]
fn multiline_math_headings_allocate_linearly() {
    assert_linear_allocations("# $x\n+y$\n", 2048);
}
