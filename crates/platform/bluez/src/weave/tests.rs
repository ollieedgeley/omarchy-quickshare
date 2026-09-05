use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

use super::{
    Assembler, Message, decode_pdu, encode_confirm, encode_data,
    encode_request, recv_data,
};
use crate::radio::{Error, ErrorKind};

#[derive(Clone, Debug)]
struct CaptureSubscriber(Arc<Mutex<Vec<CapturedEvent>>>);

#[derive(Debug)]
struct CapturedEvent {
    fields: Vec<String>,
    target: String,
}

#[derive(Default)]
struct FieldVisitor(Vec<String>);

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        self.0.push(format!("{}={value:?}", field.name()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push(format!("{}={value}", field.name()));
    }
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target() == "omarchy_quickshare::protocol"
            && *metadata.level() == tracing::Level::DEBUG
    }

    fn new_span(&self, _attributes: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        if !self.enabled(event.metadata()) {
            return;
        }
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.0.lock().expect("capture lock").push(CapturedEvent {
            fields: visitor.0,
            target: event.metadata().target().to_owned(),
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

fn close_trace(
    mut pdus: VecDeque<Result<Vec<u8>, Error>>,
) -> (ErrorKind, Vec<CapturedEvent>) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let subscriber = CaptureSubscriber(Arc::clone(&records));
    let error = tracing::subscriber::with_default(subscriber, || {
        recv_data(
            &mut Assembler::new(),
            Duration::from_secs(1),
            |_| pdus.pop_front().expect("queued pipe result"),
            |_| Ok(()),
        )
        .expect_err("pipe must close")
    });
    let events = records.lock().expect("capture lock").drain(..).collect();
    (error.kind(), events)
}

#[test]
fn request_and_confirm_match_documented_bytes() {
    assert_eq!(encode_request(), [0x80, 0x00, 0x01, 0x00, 0x01, 0x01, 0xFD]);
    assert_eq!(encode_confirm(), [0x81, 0x00, 0x01, 0x01, 0xFD]);
}

#[test]
fn data_round_trip_coalesced_and_fragmented() {
    let payload = b"hello-quick-share";
    let coalesced = encode_data(payload, 64);
    assert_eq!(coalesced.len(), 1);
    let mut assembler = Assembler::new();
    match assembler.push(&coalesced[0]).expect("coalesced") {
        Some(Message::Data(bytes)) => assert_eq!(bytes, payload),
        _ => panic!("expected data"),
    }
    let fragments = encode_data(payload, 4);
    assert!(fragments.len() > 1);
    let mut assembler = Assembler::new();
    let mut decoded = None;
    for fragment in &fragments {
        decoded = assembler.push(fragment).expect("fragment");
    }
    match decoded {
        Some(Message::Data(bytes)) => assert_eq!(bytes, payload),
        _ => panic!("expected reassembled data"),
    }
}

#[test]
fn control_pdus_do_not_extend_the_read_deadline() {
    let timeout = Duration::from_millis(3);
    let mut now = None;
    let mut pdus = VecDeque::from([
        encode_request(),
        encode_confirm(),
        encode_confirm(),
        encode_data(b"too late", 64).remove(0),
    ]);
    let error = recv_data(
        &mut Assembler::new(),
        timeout,
        |read_deadline| {
            let now = now.get_or_insert_with(|| {
                read_deadline
                    .checked_sub(timeout)
                    .expect("deadline includes timeout")
            });
            if *now >= read_deadline {
                return Err(Error::timeout());
            }
            *now += Duration::from_millis(1);
            Ok(pdus.pop_front().expect("queued PDU"))
        },
        |_| Ok(()),
    )
    .expect_err("control PDUs must not reset the deadline");

    assert_eq!(error.kind(), ErrorKind::Timeout);
}

#[test]
fn graceful_disconnect_is_closed_not_protocol_failure() {
    let error = recv_data(
        &mut Assembler::new(),
        Duration::from_secs(1),
        |_| Ok(vec![0x0C, 0x00, 0x00, 0x00, 0x08, 0x02]),
        |_| Ok(()),
    )
    .expect_err("disconnect");

    assert_eq!(error.kind(), ErrorKind::Closed);
}

#[test]
fn pipe_close_is_clean_only_between_weave_messages() {
    let boundary = recv_data(
        &mut Assembler::new(),
        Duration::from_secs(1),
        |_| Err(Error::closed()),
        |_| Ok(()),
    )
    .expect_err("boundary close");
    assert_eq!(boundary.kind(), ErrorKind::Closed);

    let mut pdus =
        VecDeque::from([Ok(vec![0x08, 0xFC, 0x9F]), Err(Error::closed())]);
    let truncated = recv_data(
        &mut Assembler::new(),
        Duration::from_secs(1),
        |_| pdus.pop_front().expect("queued pipe result"),
        |_| Ok(()),
    )
    .expect_err("close during fragmented weave message");
    assert_eq!(truncated.kind(), ErrorKind::Protocol);
}

#[test]
fn weave_close_diagnostics_distinguish_boundary_from_truncation() {
    let (boundary_kind, boundary_events) =
        close_trace(VecDeque::from([Err(Error::closed())]));
    assert_eq!(boundary_kind, ErrorKind::Closed);
    assert!(boundary_events.iter().any(|event| {
        event.target == "omarchy_quickshare::protocol"
            && event
                .fields
                .iter()
                .any(|field| field == "disconnect_origin=clean_eof")
    }));

    let (truncated_kind, truncated_events) = close_trace(VecDeque::from([
        Ok(vec![0x08, 0xFC, 0x9F]),
        Err(Error::closed()),
    ]));
    assert_eq!(truncated_kind, ErrorKind::Protocol);
    assert!(truncated_events.iter().any(|event| {
        event.target == "omarchy_quickshare::protocol"
            && event
                .fields
                .iter()
                .any(|field| field == "disconnect_origin=truncated_frame")
    }));
}

#[test]
fn malformed_lengths_and_types_fail() {
    assert_eq!(
        decode_pdu(&[]).expect_err("empty").kind(),
        ErrorKind::Protocol
    );
    assert_eq!(
        decode_pdu(&[0x80, 0x00]).expect_err("short request").kind(),
        ErrorKind::Protocol
    );
    assert_eq!(
        decode_pdu(&[0x83]).expect_err("bad cmd").kind(),
        ErrorKind::Protocol
    );
    let mut assembler = Assembler::new();
    let error = assembler
        .push(&[0x0C, 0x01, 0x02, 0x03])
        .expect_err("unknown hash");
    assert_eq!(error.kind(), ErrorKind::Protocol);
    let mut assembler = Assembler::new();
    let error = assembler.push(&[0x0C, 0xFC]).expect_err("short layer B");
    assert_eq!(error.kind(), ErrorKind::Protocol);
    let mut assembler = Assembler::new();
    let error = assembler
        .push(&[0x04, 0xFC, 0x9F, 0x5E])
        .expect_err("missing first");
    assert_eq!(error.kind(), ErrorKind::Protocol);
}
