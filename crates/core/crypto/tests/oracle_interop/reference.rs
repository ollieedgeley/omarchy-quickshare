use core::error::Error;
use std::{
    env,
    io::{BufReader, Read as _, Write as _},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use quickshare_crypto::{Handshake, SecureChannel};

const INITIATOR_RANDOM: [u8; 32] = [11; 32];
const INITIATOR_SECRET: [u8; 32] = [12; 32];
const RESPONDER_RANDOM: [u8; 32] = [13; 32];
const RESPONDER_SECRET: [u8; 32] = [14; 32];

struct Oracle {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Oracle {
    fn spawn(mode: &str) -> Result<Self, Box<dyn Error>> {
        let binary = env::var("UKEY2_SHELL")?;
        let mut child = Command::new(binary)
            .arg(format!("--mode={mode}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let input = child.stdin.take().ok_or("oracle stdin is unavailable")?;
        let output =
            child.stdout.take().ok_or("oracle stdout is unavailable")?;
        Ok(Self {
            child,
            input,
            output: BufReader::new(output),
        })
    }

    fn send(&mut self, message: &[u8]) -> Result<(), Box<dyn Error>> {
        let length = u32::try_from(message.len())?;
        self.input.write_all(&length.to_be_bytes())?;
        self.input.write_all(message)?;
        self.input.flush()?;
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut header = [0; 4];
        self.output.read_exact(&mut header)?;
        let length = usize::try_from(u32::from_be_bytes(header))?;
        let mut message = vec![0; length];
        self.output.read_exact(&mut message)?;
        Ok(message)
    }

    fn command(
        &mut self,
        command: &[u8],
        payload: &[u8],
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut message = command.to_vec();
        message.extend_from_slice(payload);
        self.send(&message)?;
        self.receive()
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

fn verify_d2d(
    channel: &mut SecureChannel,
    oracle: &mut Oracle,
    rust_payload: &[u8],
    oracle_payload: &[u8],
) -> Result<(), Box<dyn Error>> {
    let encrypted = channel
        .encrypt(rust_payload, [21; 16])
        .expect("Rust encrypts a D2D frame");
    assert_eq!(oracle.command(b"decrypt ", &encrypted)?, rust_payload);

    let encrypted = oracle.command(b"encrypt ", oracle_payload)?;
    assert_eq!(
        channel
            .decrypt(&encrypted)
            .expect("Rust decrypts the Google D2D frame"),
        oracle_payload
    );

    assert_eq!(
        oracle.command(b"session_unique ", &[])?,
        channel.session_unique()
    );
    Ok(())
}

fn rust_initiator() -> Result<(), Box<dyn Error>> {
    let mut oracle = Oracle::spawn("responder")?;
    let mut handshake =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);

    oracle.send(&handshake.next_message().expect("Rust starts UKEY2"))?;
    handshake
        .receive(&oracle.receive()?)
        .expect("Rust accepts Google's server init");
    oracle.send(&handshake.next_message().expect("Rust finishes UKEY2"))?;
    drop(oracle.receive()?);
    oracle.send(b"ok")?;

    verify_d2d(
        &mut handshake.into_channel().expect("Rust completes UKEY2"),
        &mut oracle,
        b"Rust initiator encrypted D2D frame",
        b"Google responder encrypted D2D frame",
    )
}

fn rust_responder() -> Result<(), Box<dyn Error>> {
    let mut oracle = Oracle::spawn("initiator")?;
    let mut handshake =
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);

    handshake
        .receive(&oracle.receive()?)
        .expect("Rust accepts Google's client init");
    oracle.send(&handshake.next_message().expect("Rust sends server init"))?;
    handshake
        .receive(&oracle.receive()?)
        .expect("Rust accepts Google's client finish");
    drop(oracle.receive()?);
    oracle.send(b"ok")?;

    verify_d2d(
        &mut handshake.into_channel().expect("Rust completes UKEY2"),
        &mut oracle,
        b"Rust responder encrypted D2D frame",
        b"Google initiator encrypted D2D frame",
    )
}

#[test]
fn rust_and_google_oracle_interoperate_in_both_ukey2_roles()
-> Result<(), Box<dyn Error>> {
    rust_initiator()?;
    rust_responder()
}
