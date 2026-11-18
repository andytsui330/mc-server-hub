use mc_hub_core::automation::{Action, Policy, Runtime};
use serde_json::json;
#[test]
fn restart_is_bounded_and_manual_stop_cancels() {
    let p = Policy::parse(&json!({"crash_enabled":true,"crash_limit":2,"crash_delay":5})).unwrap();
    let mut r = Runtime::default();
    r.exited(&p, 1, 0);
    assert!(r.tick(&p, 4999, false, false).is_empty());
    assert_eq!(r.tick(&p, 5000, false, true), vec![Action::Start]);
    r.exited(&p, 1, 6000);
    assert_eq!(r.tick(&p, 11000, false, true), vec![Action::Start]);
    r.exited(&p, 1, 12000);
    assert!(r.tick(&p, 99999, false, true).is_empty());
    r.manual_start();
    r.exited(&p, 1, 0);
    r.cancel();
    assert!(r.tick(&p, 10000, false, true).is_empty());
    r.manual_start();
    r.exited(&p, 0, 0);
    assert!(r.tick(&p, 10000, false, true).is_empty());
}
#[test]
fn scheduled_restart_broadcasts_stops_and_waits_for_clean_exit() {
    let p = Policy::parse(&json!({"schedule_enabled":true,"schedule_time":"04:00","countdown":30}))
        .unwrap();
    let mut r = Runtime::default();
    assert_eq!(r.schedule(&p, 0, true), vec![Action::Broadcast(30)]);
    assert!(r.tick(&p, 10000, true, false).is_empty());
    assert_eq!(r.tick(&p, 30000, true, false), vec![Action::Stop]);
    assert!(r.tick(&p, 40000, true, true).is_empty());
    r.exited(&p, 0, 41000);
    assert!(r.tick(&p, 42000, false, false).is_empty());
    assert_eq!(r.tick(&p, 43000, false, true), vec![Action::Start]);
    let mut r = Runtime::default();
    r.schedule(&p, 0, true);
    r.tick(&p, 30000, true, false);
    r.exited(&p, 1, 40000);
    assert!(r.tick(&p, 90000, false, true).is_empty());
    assert!(Policy::parse(&json!({"crash_limit":0})).is_err());
    assert!(Policy::parse(&json!({"schedule_time":"25:00"})).is_err());
}
