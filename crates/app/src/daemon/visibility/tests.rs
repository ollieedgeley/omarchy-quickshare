mod fixture;

pub(crate) use fixture::Fixture;

use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;
use core::time::Duration;
use std::sync::mpsc;
use std::time::Instant;

use quickshare_control::request::Envelope as Command;
use quickshare_control::response::Response;
use quickshare_sharing::{Phase, VisibilityState};

use super::Resources;
use crate::config::Config;
use crate::daemon::network::NetworkEvent;

#[test]
fn temporary_admission_closes_at_600_boot_seconds_without_renewal() {
    let mut fixture = Fixture::new(Config::default());
    let opened = fixture.open();
    assert_eq!(opened.visibility_status().remaining_secs, Some(600));
    fixture.clock().advance(599);
    let _already_open = fixture.open();
    assert_eq!(
        fixture.snapshot().visibility_status().remaining_secs,
        Some(1)
    );
    let admitted = fixture.offer();
    assert!(matches!(
        fixture.command(Command::reject(admitted)).response(),
        Response::Applied
    ));
    assert!(matches!(
        fixture.command(Command::dismiss(admitted)).response(),
        Response::Applied
    ));
    fixture.clock().advance(1);
    assert!(matches!(
        fixture
            .command(Command::simulate_incoming_text("too late"))
            .response(),
        Response::NotFound
    ));
    let closed = fixture.snapshot();
    assert_eq!(closed.visibility(), VisibilityState::Closed);
    assert_eq!(closed.visibility_status().remaining_secs, None);
    assert_eq!(fixture.released(), 1);
}

#[test]
fn passive_expiry_preserves_admitted_consent_until_its_captured_deadline() {
    let mut fixture = Fixture::new(Config {
        consent_timeout_secs: 20,
        ..Config::default()
    });
    let _opened = fixture.open();
    fixture.clock().advance(590);
    let share_id = fixture.offer();
    fixture.configure(false, 1);
    fixture.clock().advance(10);
    let expired = fixture.snapshot();
    assert_eq!(expired.visibility(), VisibilityState::Closed);
    assert_eq!(
        expired.active_share().expect("pending offer").phase(),
        Phase::AwaitingLocalConsent
    );
    fixture.clock().advance(9);
    assert!(matches!(
        fixture.command(Command::accept(share_id)).response(),
        Response::Applied
    ));
    assert_eq!(
        fixture
            .snapshot()
            .active_share()
            .expect("accepted share")
            .phase(),
        Phase::Transferring
    );
}

#[test]
fn delayed_accept_cannot_bypass_captured_consent_timeout() {
    let mut fixture = Fixture::new(Config {
        consent_timeout_secs: 20,
        ..Config::default()
    });
    let _opened = fixture.open();
    fixture.clock().advance(590);
    let share_id = fixture.offer();
    fixture.configure(false, 900);
    fixture.clock().advance(20);
    assert!(matches!(
        fixture.command(Command::accept(share_id)).response(),
        Response::NotFound
    ));
    let snapshot = fixture.snapshot();
    let share = snapshot.active_share().expect("timed out consent");
    assert_eq!(share.phase(), Phase::Failed);
    assert_eq!(share.terminal_reason(), Some("timed_out"));
}

#[test]
fn stop_and_durable_off_cancel_pending_but_not_saved_off_policy() {
    let mut temporary = Fixture::new(Config::default());
    let _opened = temporary.open();
    let share_id = temporary.offer();
    let _closed = temporary.command(Command::close_visibility());
    let stopped = temporary.snapshot();
    assert!(!stopped.visibility_status().discoverable);
    assert_eq!(
        stopped.active_share().expect("cancelled pending").phase(),
        Phase::Cancelled
    );
    assert!(matches!(
        temporary.command(Command::accept(share_id)).response(),
        Response::NotFound
    ));
    assert_eq!(temporary.released(), 1);

    let mut durable = Fixture::new(Config {
        discoverable: true,
        ..Config::default()
    });
    let _durable_opened = durable
        .wait_for(|snapshot| snapshot.visibility() == VisibilityState::Open);
    let durable_share_id = durable.offer();
    durable.configure(false, 300);
    let durable_stopped = durable.snapshot();
    assert_eq!(durable_stopped.visibility(), VisibilityState::Closed);
    assert!(!durable_stopped.visibility_status().temporary);
    assert_eq!(
        durable_stopped
            .active_share()
            .expect("revoked consent")
            .phase(),
        Phase::Cancelled
    );
    assert!(matches!(
        durable
            .command(Command::accept(durable_share_id))
            .response(),
        Response::NotFound
    ));
}

#[test]
fn durable_on_upgrades_temporary_lease_without_interrupting_accepted_share() {
    let mut fixture = Fixture::new(Config::default());
    let _opened = fixture.open();
    let share_id = fixture.offer();
    assert!(matches!(
        fixture.command(Command::accept(share_id)).response(),
        Response::Applied
    ));
    fixture.clock().advance(599);
    fixture.configure(true, 300);
    fixture.clock().advance(10_000);
    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.visibility(), VisibilityState::Open);
    assert_eq!(snapshot.visibility_status().remaining_secs, None);
    assert!(!snapshot.visibility_status().temporary);
    assert!(snapshot.visibility_status().discoverable);
    assert_eq!(
        snapshot.active_share().expect("accepted share").phase(),
        Phase::Transferring
    );
}

#[test]
fn daemon_restart_restores_only_durable_policy() {
    let mut first = Fixture::new(Config::default());
    let _opened = first.open();
    drop(first);
    let mut restarted = Fixture::new(Config::default());
    assert_eq!(restarted.snapshot().visibility(), VisibilityState::Closed);
    drop(restarted);
    for _ in 0..2 {
        let mut durable = Fixture::new(Config {
            discoverable: true,
            ..Config::default()
        });
        let snapshot = durable.wait_for(|snapshot| {
            snapshot.visibility() == VisibilityState::Open
        });
        assert_eq!(snapshot.visibility_status().remaining_secs, None);
    }
}

#[test]
fn delayed_activation_starts_countdown_only_once_after_success() {
    let released = Arc::new(AtomicUsize::new(0));
    let leases = Arc::clone(&released);
    let (started, starting) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let mut fixture =
        Fixture::with_activation(Config::default(), released, move |_| {
            started.send(()).expect("activation started");
            resumed
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())?;
            Ok(Resources::fake(Arc::clone(&leases), None))
        });
    let _requested = fixture.command(Command::open_visibility());
    starting
        .recv_timeout(Duration::from_secs(2))
        .expect("activation requested");
    fixture.clock().advance(800);
    let _repeated = fixture.command(Command::open_visibility());
    let pending_activation = fixture.snapshot();
    assert_eq!(pending_activation.visibility(), VisibilityState::Starting);
    assert_eq!(pending_activation.visibility_status().remaining_secs, None);
    assert!(matches!(
        fixture
            .command(Command::simulate_incoming_text("not active"))
            .response(),
        Response::NotFound
    ));
    resume.send(()).expect("finish activation");
    let open = fixture
        .wait_for(|snapshot| snapshot.visibility() == VisibilityState::Open);
    assert_eq!(open.visibility_status().remaining_secs, Some(600));
    assert_eq!(
        open.visibility_status().available_media,
        [String::from("wifi_lan")]
    );
    fixture.clock().advance(600);
    assert_eq!(fixture.snapshot().visibility(), VisibilityState::Closed);
    assert_eq!(fixture.released(), 1);
}

#[test]
fn failed_partial_activation_cleans_leases_and_never_admits() {
    let released = Arc::new(AtomicUsize::new(0));
    let leases = Arc::clone(&released);
    let mut fixture =
        Fixture::with_activation(Config::default(), released, move |_| {
            let _partial = Resources::fake(Arc::clone(&leases), None);
            Err(String::from("advertisement registration failed"))
        });
    let _requested = fixture.command(Command::open_visibility());
    let failed = fixture.wait_for(|snapshot| {
        snapshot.visibility() == VisibilityState::Unavailable
    });
    assert!(!failed.visibility_status().requested);
    assert_eq!(failed.visibility_status().remaining_secs, None);
    assert!(failed.visibility_status().available_media.is_empty());
    assert!(failed.visibility_status().error.is_some());
    assert!(matches!(
        fixture
            .command(Command::simulate_incoming_text("denied"))
            .response(),
        Response::NotFound
    ));
    assert_eq!(fixture.released(), 1);
}

#[test]
fn stop_during_activation_releases_late_success_without_reopening() {
    let released = Arc::new(AtomicUsize::new(0));
    let leases = Arc::clone(&released);
    let (started, starting) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let mut fixture =
        Fixture::with_activation(Config::default(), released, move |_| {
            started.send(()).expect("activation started");
            resumed
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())?;
            Ok(Resources::fake(Arc::clone(&leases), None))
        });
    let _requested = fixture.command(Command::open_visibility());
    starting
        .recv_timeout(Duration::from_secs(2))
        .expect("activation pending");
    let _closed = fixture.command(Command::close_visibility());
    resume.send(()).expect("late activation success");
    let deadline = Instant::now() + Duration::from_secs(2);
    while fixture.released() == 0 {
        assert!(Instant::now() < deadline, "late leases were not cleaned");
        std::thread::yield_now();
    }
    let stopped = fixture.snapshot();
    assert_eq!(stopped.visibility(), VisibilityState::Closed);
    assert!(!stopped.visibility_status().requested);
    assert!(stopped.visibility_status().available_media.is_empty());
}

#[test]
fn queued_offer_is_not_admitted_after_suspend_crosses_expiry() {
    let mut fixture = Fixture::new(Config::default());
    let _opened = fixture.open();
    fixture.clock().advance(599);
    let generation = fixture.visibility().generation_if_open().expect("open");
    let consent = fixture
        .visibility()
        .propose(generation, Duration::from_secs(300))
        .expect("offer before expiry");
    fixture
        .events()
        .send(NetworkEvent::InboundOffered {
            consent,
            kind: quickshare_sharing::OfferKind::Text,
            name: String::from("queued"),
            size_bytes: 4,
            verification_code: String::from("1234"),
        })
        .expect("queued offer");
    fixture.clock().advance(3600);
    let resumed = fixture.snapshot();
    assert_eq!(resumed.visibility(), VisibilityState::Closed);
    assert!(resumed.active_share().is_none());
    assert_eq!(fixture.released(), 1);
}

#[test]
fn unassigned_old_connection_failure_cannot_end_admitted_consent() {
    let mut fixture = Fixture::new(Config::default());
    let _opened = fixture.open();
    let share_id = fixture.offer();
    fixture
        .events()
        .send(NetworkEvent::InboundFailed {
            reason: String::from("timed_out"),
            share_id: None,
            consent: None,
        })
        .expect("unassigned connection error");
    let snapshot = fixture.snapshot();
    let share = snapshot.active_share().expect("current consent retained");
    assert_eq!(share.id().get(), share_id);
    assert_eq!(share.phase(), Phase::AwaitingLocalConsent);
}
