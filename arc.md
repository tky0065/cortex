# ARC (Atomic Reference Counted) in Rust

## Overview
`Arc<T>` stands for **Atomic Reference Counted**. It is a thread‑safe smart pointer that enables multiple ownership of a value by keeping a count of how many `Arc` pointers refer to the same data. When the count drops to zero, the value is automatically dropped.

`Arc` is the multi‑threaded counterpart of `Rc<T>` (Reference Counted). The key difference is that `Arc` uses atomic operations for the reference count, making it safe to share across threads, whereas `Rc` is only safe for single‑threaded use.

## When to Use `Arc`
- You need shared ownership of data across multiple threads.
- The data is immutable (or you protect mutation with interior mutability primitives like `Mutex`, `RwLock`, or `RefCell` inside the `Arc`).
- You want automatic cleanup when the last reference goes away.

## Basic Syntax
```rust
use std::sync::Arc;

fn main() {
    // Create an Arc wrapping a value
    let data = Arc::new(String::from("hello"));

    // Clone the Arc to create another reference
    let data_clone = Arc::clone(&data);

    // Both `data` and `data_clone` point to the same allocation
    assert!(Arc::ptr_eq(&data, &data_clone));

    // When the last Arc is dropped, the inner String is dropped
}
```

## Cloning `Arc`
Cloning an `Arc` does **not** clone the inner value; it only increments the reference count atomically:
```rust
let a = Arc::new(5);
let b = Arc::clone(&a); // increments count
assert_eq!(Arc::strong_count(&a), 2);
```

## Interior Mutability
Since `Arc` provides immutable shared access, to mutate the data you typically combine it with a synchronization primitive:
```rust
use std::sync::{Arc, Mutex};

let counter = Arc::new(Mutex::new(0));
let handles: Vec<_> = (0..10)
    .map({
        let counter = Arc::clone(&counter);
        move |_| {
            let mut num = counter.lock().unwrap();
            *num += 1;
        }
    })
    .collect();

for h in handles {
    h.join().unwrap();
}

println!("Result: {}", *counter.lock().unwrap()); // 10
```

## Performance Considerations
- **Atomic operations**: Incrementing/decrementing the count uses CPU atomic instructions, which are more expensive than the non‑atomic increments used by `Rc`. Use `Arc` only when thread safety is required.
- **Cache locality**: All `Arc` instances point to the same allocation, so reading the inner value is cheap.
- **Avoid unnecessary clones**: Cloning an `Arc` is cheap (just an atomic increment), but if you don’t need shared ownership, prefer owning the value directly or using references.

## Comparison with `Rc`
| Feature                | `Rc<T>`                     | `Arc<T>`                     |
|------------------------|-----------------------------|------------------------------|
| Thread safety          | ❌ Not safe across threads   | ✅ Safe (uses atomics)       |
| Reference count type   | `Cell<usize>` (non‑atomic)  | `AtomicUsize`                |
| Overhead               | Lower (no atomic ops)       | Slightly higher              |
| Typical use case       | Single‑threaded shared ownership | Multi‑threaded shared ownership |

## Limitations
- `Arc` only provides **immutable** access to the inner data by default. To mutate, you must pair it with a synchronization primitive (`Mutex`, `RwLock`, etc.).
- Cyclic references (e.g., two `Arc`s pointing to each other) will cause a memory leak because the reference count never reaches zero. Use `Weak` to break cycles when needed.

## Example: Breaking a Cycle with `Weak`
```rust
use std::sync::{Arc, Weak};

struct Node {
    value: i32,
    parent: Option<Weak<Node>>,
    children: Vec<Arc<Node>>,
}

fn main() {
    let leaf = Arc::new(Node {
        value: 3,
        parent: None,
        children: vec![],
    });

    let branch = Arc::new(Node {
        value: 5,
        parent: Some(Arc::downgrade(&leaf)), // Weak reference
        children: vec![Arc::clone(&leaf)],
    });

    // `leaf.parent` is a Weak; upgrading gives an Option<Arc<Node>>
    if let Some(parent) = leaf.parent.as_ref().and_then(|w| w.upgrade()) {
        println!("Leaf's parent value: {}", parent.value);
    }
}
```

## Summary
- Use `Arc<T>` when you need **thread‑safe shared ownership** of data.
- It atomically tracks the number of references; the data is dropped when the last `Arc` is gone.
- Combine with interior mutability (`Mutex`, `RwLock`, etc.) for mutable shared state.
- Prefer `Rc<T>` for single‑threaded scenarios to avoid the atomic overhead.
- Be aware of reference cycles; use `Weak` to break them when necessary.
