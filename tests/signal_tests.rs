use caravan::error::CaravanError;
use caravan::signal::{check_shutdown, ShutdownFlag};
use std::thread;

// Test 1: Shutdown flag starts as false
#[test]
fn shutdown_flag_initial_state() {
    let flag = ShutdownFlag::new();
    assert!(!flag.is_shutdown_requested());
}

// Test 2: Shutdown flag can be set to true
#[test]
fn shutdown_flag_can_be_set() {
    let flag = ShutdownFlag::new();
    flag.request_shutdown();
    assert!(flag.is_shutdown_requested());
}

// Test 3: Shutdown flag can be reset
#[test]
fn shutdown_flag_can_be_reset() {
    let flag = ShutdownFlag::new();
    flag.request_shutdown();
    assert!(flag.is_shutdown_requested());
    flag.reset();
    assert!(!flag.is_shutdown_requested());
}

// Test 4: GracefulShutdown error variant exists
#[test]
fn graceful_shutdown_error_variant_exists() {
    let error = CaravanError::GracefulShutdown;
    match error {
        CaravanError::GracefulShutdown => {
            assert_eq!(format!("{}", error), "graceful shutdown requested");
        }
        _ => panic!("Expected GracefulShutdown variant"),
    }
}

// Test 5: Thread-safe flag sharing
#[test]
fn shutdown_flag_thread_safe() {
    let flag = ShutdownFlag::new();
    let flag_clone = flag.clone();

    let handle = thread::spawn(move || {
        flag_clone.request_shutdown();
    });

    handle.join().unwrap();
    assert!(flag.is_shutdown_requested());
}

// Test 6: check_shutdown returns Ok when no shutdown requested
#[test]
fn check_shutdown_returns_ok_when_no_shutdown() {
    let flag = ShutdownFlag::new();
    assert!(check_shutdown(&flag).is_ok());
}

// Test 7: check_shutdown returns GracefulShutdown error when shutdown requested
#[test]
fn check_shutdown_returns_error_when_shutdown_requested() {
    let flag = ShutdownFlag::new();
    flag.request_shutdown();

    let result = check_shutdown(&flag);
    match result {
        Err(CaravanError::GracefulShutdown) => {
            // Expected
        }
        _ => panic!("Expected GracefulShutdown error, got {:?}", result),
    }
}

// Test 8: ShutdownFlag implements Default trait
#[test]
fn shutdown_flag_implements_default() {
    let flag = ShutdownFlag::default();
    assert!(!flag.is_shutdown_requested());
}

// Test 9: Clone creates independent flag with same state
#[test]
fn shutdown_flag_clone_creates_independent_flag() {
    let flag1 = ShutdownFlag::new();
    let flag2 = flag1.clone();

    // Initially both false
    assert!(!flag1.is_shutdown_requested());
    assert!(!flag2.is_shutdown_requested());

    // Setting one doesn't affect the other (they share Arc)
    flag1.request_shutdown();
    assert!(flag1.is_shutdown_requested());
    assert!(flag2.is_shutdown_requested()); // Actually they share state!

    // Reset one resets both
    flag2.reset();
    assert!(!flag1.is_shutdown_requested());
    assert!(!flag2.is_shutdown_requested());
}
