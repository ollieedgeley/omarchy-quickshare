use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

use super::Credentials;
use crate::Error;

pub(super) type Settings = HashMap<String, HashMap<String, OwnedValue>>;

pub(super) fn wireless(
    id: &str,
    mode: &str,
    credentials: &Credentials,
    ipv4_method: &str,
) -> Result<Settings, Error> {
    let mut connection = HashMap::new();
    let _previous_id = connection
        .insert(String::from("id"), owned(Value::from(id.to_owned()))?);
    let _previous_type = connection
        .insert(String::from("type"), owned(Value::from("802-11-wireless"))?);
    let _previous_autoconnect = connection
        .insert(String::from("autoconnect"), owned(Value::from(false))?);
    let mut wireless = HashMap::new();
    let _previous_mode = wireless
        .insert(String::from("mode"), owned(Value::from(mode.to_owned()))?);
    let _previous_ssid = wireless.insert(
        String::from("ssid"),
        owned(Value::from(credentials.ssid.as_bytes().to_vec()))?,
    );
    if let Some(frequency) = credentials.frequency {
        let _previous_channel = wireless
            .insert(String::from("channel"), owned(Value::from(frequency))?);
    }
    let mut security = HashMap::new();
    let _previous_key_mgmt = security
        .insert(String::from("key-mgmt"), owned(Value::from("wpa-psk"))?);
    let _previous_psk = security.insert(
        String::from("psk"),
        owned(Value::from(credentials.password.clone()))?,
    );
    let mut ipv4 = HashMap::new();
    let _previous_method = ipv4.insert(
        String::from("method"),
        owned(Value::from(ipv4_method.to_owned()))?,
    );
    Ok(HashMap::from([
        (String::from("connection"), connection),
        (String::from("802-11-wireless"), wireless),
        (String::from("802-11-wireless-security"), security),
        (String::from("ipv4"), ipv4),
    ]))
}

pub(super) fn p2p(
    id: &str,
    peer: Option<&str>,
    credentials: &Credentials,
) -> Result<Settings, Error> {
    let mut connection = HashMap::new();
    let _previous_id = connection
        .insert(String::from("id"), owned(Value::from(id.to_owned()))?);
    let _previous_type = connection
        .insert(String::from("type"), owned(Value::from("wifi-p2p"))?);
    let _previous_autoconnect = connection
        .insert(String::from("autoconnect"), owned(Value::from(false))?);
    let mut p2p = HashMap::new();
    let _previous_wps =
        p2p.insert(String::from("wps-method"), owned(Value::from(1_u32))?);
    if let Some(peer) = peer {
        let _previous_peer = p2p
            .insert(String::from("peer"), owned(Value::from(peer.to_owned()))?);
    }
    let mut ipv4 = HashMap::new();
    let _previous_method =
        ipv4.insert(String::from("method"), owned(Value::from("auto"))?);
    let mut settings = HashMap::from([
        (String::from("connection"), connection),
        (String::from("wifi-p2p"), p2p),
        (String::from("ipv4"), ipv4),
    ]);
    if !credentials.ssid.is_empty() {
        let mut wireless = HashMap::new();
        let _previous_ssid = wireless.insert(
            String::from("ssid"),
            owned(Value::from(credentials.ssid.as_bytes().to_vec()))?,
        );
        let _previous_wireless =
            settings.insert(String::from("802-11-wireless"), wireless);
    }
    Ok(settings)
}

fn owned(value: Value<'static>) -> Result<OwnedValue, Error> {
    OwnedValue::try_from(value).map_err(|error| Error(error.to_string()))
}
